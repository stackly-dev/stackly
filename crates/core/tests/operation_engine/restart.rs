//! Recovery after a crash, resume, abandon, and shutdown (spec 0007 AC-10,
//! AC-11, AC-14).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use stackly_core::operation::testing::{EffectCounter, HoldHandle, ScriptedFactory, ScriptedWork};
use stackly_core::operation::{
    BeginError, BoxFuture, CancelDisposition, CompensationError, CompensationResult,
    CompensationScope, Compensator, EngineError, NotAbandonableReason, NotResumableReason,
    OperationContext, OperationOutcome, OperationWork, RecoveryAction, StepSpec, WorkError,
    WorkOutcome, WorkResult,
};
use stackly_domain::{
    CreatedRecord, CreatedState, EnvironmentId, LockKey, OperationKind, OperationResource,
    OperationState, RecoveryCause, RecoveryHint, Registry, RuntimeId, StepState, WriteBatch,
    WriteOp,
};
use stackly_testkit::EnvironmentBuilder;

use crate::common::{Harness, TestError, fast, manager_request, request, wait_state};

fn environment(harness: &Harness) -> EnvironmentId {
    let builder = EnvironmentBuilder::new().no_members();
    let id = builder.id();
    harness
        .registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            builder.build(),
        )]))
        .expect("test environment can be inserted");
    id
}

fn install(work: ScriptedWork) -> stackly_core::operation::BeginRequest {
    let mut begin = request(
        OperationKind::InstallRuntime,
        OperationResource::Runtime(RuntimeId::generate()),
        work.boxed(),
    );
    begin.creates_resource = true;
    begin
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_operation_interrupted_in_a_repeatable_step_resumes_after_a_restart() {
    let harness = Harness::new().await;
    let stuck = HoldHandle::new();
    let id = harness
        .begin(install(
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .hold(&stuck),
        ))
        .await;
    stuck.started().await;

    let (engine, _) = harness.restarted(fast());
    let effects = EffectCounter::new();
    engine.register_factory(
        OperationKind::InstallRuntime,
        Arc::new(ScriptedFactory::always(
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .effects(&effects),
        )),
    );
    let report = engine.recover().await.unwrap();
    assert_eq!(report.resumed, 1);
    assert_eq!(engine.stats().interrupted_at_start, 1);
    engine.start();
    let done = wait_state(&engine, id, OperationState::Succeeded).await;

    assert_eq!(done.steps.len(), 1);
    assert_eq!(
        (done.steps[0].state(), done.steps[0].attempts()),
        (StepState::Done, 2)
    );
    assert_eq!(effects.get(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_interrupted_operation_that_cannot_resume_is_compensated_or_needs_recovery() {
    let harness = Harness::new().await;
    let before = HoldHandle::new();
    let early = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("create")
                .creates_instance_tree()
                .step("wait")
                .hold(&before)
                .boxed(),
        ))
        .await;
    before.started().await;
    let env = environment(&harness);
    let after = HoldHandle::new();
    let late = harness
        .begin(request(
            OperationKind::RemoveInstance,
            OperationResource::Environment(env),
            ScriptedWork::new()
                .step("swap")
                .irreversible()
                .step("wait")
                .hold(&after)
                .boxed(),
        ))
        .await;
    after.started().await;
    let waiter = harness
        .begin(request(
            OperationKind::RemoveInstance,
            OperationResource::Environment(env),
            ScriptedWork::new().step("swap").boxed(),
        ))
        .await;
    let instances_before: Vec<_> = std::fs::read_dir(harness.layout.layout().instances_dir())
        .unwrap()
        .collect();
    assert_eq!(instances_before.len(), 1);

    let (engine, _) = harness.restarted(fast());
    engine.register_factory(
        OperationKind::RemoveInstance,
        Arc::new(ScriptedFactory::always(ScriptedWork::new().step("swap"))),
    );
    let report = engine.recover().await.unwrap();
    assert_eq!(
        (
            report.auto_compensating,
            report.needs_recovery,
            report.closed_for_recovery
        ),
        (1, 1, 1)
    );
    let manual = engine.get(late).await.unwrap().unwrap();
    assert_eq!(
        manual.view.operation.recovery_hint(),
        Some(RecoveryHint::Manual)
    );
    assert_eq!(
        manual.view.journal.recovery_cause(),
        Some(RecoveryCause::Interrupted)
    );
    assert_eq!(manual.view.recovery_actions, [RecoveryAction::Abandon]);
    assert!(matches!(
        engine.locks().holder(&LockKey::Environment(env)),
        Some(stackly_core::operation::LockHolder::Operation { id, state: OperationState::NeedsRecovery }) if id == late
    ));
    let closed = engine.get(waiter).await.unwrap().unwrap();
    assert_eq!(closed.view.operation.state(), OperationState::Failed);
    assert_eq!(
        closed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("operation.recovery_required")
    );
    assert!(matches!(
        engine
            .begin(request(
                OperationKind::RemoveInstance,
                OperationResource::Environment(env),
                ScriptedWork::new().step("swap").boxed(),
            ))
            .await,
        Err(BeginError::RecoveryRequired { operation, .. }) if operation == late
    ));

    engine.start();
    let compensated = wait_state(&engine, early, OperationState::Failed).await;
    let failure = compensated.view.operation.failure().unwrap();
    assert_eq!(
        (failure.code().as_str(), failure.retryable()),
        ("interrupted", true)
    );
    assert_eq!(
        compensated
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("operation.interrupted")
    );
    let left: Vec<_> = std::fs::read_dir(harness.layout.layout().instances_dir())
        .unwrap()
        .collect();
    assert!(left.is_empty(), "the interrupted create was compensated");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_operations_are_rebuilt_after_a_restart_or_closed_without_a_factory() {
    let harness = Harness::with_limits(fast().with_slots(1, 64)).await;
    let blocker = HoldHandle::new();
    let blocking = harness
        .begin(request(
            OperationKind::EnvironmentDown,
            OperationResource::Environment(environment(&harness)),
            ScriptedWork::new()
                .step("drain")
                .repeatable()
                .hold(&blocker)
                .boxed(),
        ))
        .await;
    blocker.started().await;
    let rebuilt = harness
        .begin(request(
            OperationKind::EnvironmentDown,
            OperationResource::Environment(environment(&harness)),
            ScriptedWork::new().step("drain").boxed(),
        ))
        .await;
    let orphan = harness
        .begin(request(
            OperationKind::EnvironmentUp,
            OperationResource::Environment(environment(&harness)),
            ScriptedWork::new().step("start").boxed(),
        ))
        .await;

    let (engine, _) = harness.restarted(fast());
    engine.register_factory(
        OperationKind::EnvironmentDown,
        Arc::new(ScriptedFactory::always(
            ScriptedWork::new().step("drain").repeatable(),
        )),
    );
    let report = engine.recover().await.unwrap();
    assert_eq!(
        (report.resumed, report.queued, report.rebuild_failed),
        (1, 1, 1)
    );
    let closed = engine.get(orphan).await.unwrap().unwrap();
    assert_eq!(closed.view.operation.state(), OperationState::Failed);
    assert_eq!(
        closed
            .view
            .operation
            .failure()
            .map(|f| (f.code().as_str(), f.retryable())),
        Some(("interrupted", true))
    );
    engine.start();
    wait_state(&engine, blocking, OperationState::Succeeded).await;
    wait_state(&engine, rebuilt, OperationState::Succeeded).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_reruns_the_failed_step_and_refuses_what_cannot_resume() {
    let harness = Harness::new().await;
    harness.engine.register_factory(
        OperationKind::InstallRuntime,
        Arc::new(ScriptedFactory::always(
            ScriptedWork::new()
                .step("fetch")
                .irreversible()
                .step("verify")
                .repeatable(),
        )),
    );
    let id = harness
        .begin(install(
            ScriptedWork::new()
                .step("fetch")
                .irreversible()
                .step("verify")
                .repeatable()
                .fail("runtime.checksum_mismatch", true),
        ))
        .await;
    let stuck = wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;
    assert_eq!(
        stuck.view.operation.recovery_hint(),
        Some(RecoveryHint::Resume)
    );
    assert_eq!(
        stuck.view.recovery_actions,
        [RecoveryAction::Resume, RecoveryAction::Abandon]
    );
    assert_eq!(
        stuck.view.journal.recovery_cause(),
        Some(RecoveryCause::StepFailed)
    );

    let wrong = stuck.view.operation.revision().next();
    assert!(matches!(
        harness.engine.resume(id, Some(wrong)).await,
        Err(EngineError::RevisionMismatch { .. })
    ));
    let resumed = harness
        .engine
        .resume(id, Some(stuck.view.operation.revision()))
        .await
        .unwrap();
    assert_eq!(resumed.journal.recovery_cause(), None);
    let done = wait_state(&harness.engine, id, OperationState::Succeeded).await;
    let attempts: Vec<(&str, u32)> = done
        .steps
        .iter()
        .map(|step| (step.name().as_str(), step.attempts()))
        .collect();
    assert_eq!(attempts, [("fetch", 1), ("verify", 2)]);

    assert!(matches!(
        harness.engine.resume(id, None).await,
        Err(EngineError::NotResumable {
            reason: NotResumableReason::WrongState,
            ..
        })
    ));
    let manual = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("swap")
                .irreversible()
                .step("verify")
                .fail("manager.verify_failed", false)
                .boxed(),
        ))
        .await;
    wait_state(&harness.engine, manual, OperationState::NeedsRecovery).await;
    assert!(matches!(
        harness.engine.resume(manual, None).await,
        Err(EngineError::NotResumable {
            reason: NotResumableReason::KindNotResumable,
            ..
        })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn abandon_undoes_reversible_steps_and_closes_the_operation_with_its_leftovers() {
    let harness = Harness::new().await;
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .creates_instance_tree()
                .step("swap")
                .irreversible()
                .creates_instance_tree()
                .step("verify")
                .fail("manager.verify_failed", false)
                .boxed(),
        ))
        .await;
    let stuck = wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;
    assert_eq!(stuck.created.len(), 2);

    let stale = harness
        .engine
        .abandon(id, stuck.view.operation.revision().next())
        .await;
    assert!(matches!(stale, Err(EngineError::RevisionMismatch { .. })));
    let abandoned = harness
        .engine
        .abandon(id, stuck.view.operation.revision())
        .await
        .unwrap();

    assert_eq!(
        abandoned
            .view
            .operation
            .failure()
            .map(|f| f.code().as_str()),
        Some("abandoned")
    );
    assert_eq!(
        abandoned.leftovers.len(),
        1,
        "the irreversible step's item stays"
    );
    assert_eq!(
        abandoned.leftovers[0].state(),
        stackly_domain::CreatedState::Created
    );
    let outcome = stackly_core::operation::OperationOutcome::from_payload(
        abandoned.view.journal.result().unwrap(),
    )
    .unwrap();
    let outcomes: Vec<_> = outcome.partial.iter().map(|item| item.outcome).collect();
    assert_eq!(
        outcomes,
        [
            stackly_core::operation::PartialOutcome::Done,
            stackly_core::operation::PartialOutcome::LeftOver
        ]
    );
    assert!(matches!(
        harness
            .engine
            .abandon(id, abandoned.view.operation.revision())
            .await,
        Err(EngineError::NotAbandonable {
            reason: NotAbandonableReason::WrongState,
            ..
        })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_parks_resumable_work_stops_the_rest_and_ends_within_its_grace() {
    let harness = Harness::new().await;
    let parked_hold = HoldHandle::new();
    let parked = harness
        .begin(install(
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .hold(&parked_hold),
        ))
        .await;
    let stopped_hold = HoldHandle::new();
    let stopped = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .creates_instance_tree()
                .step("wait")
                .hold(&stopped_hold)
                .boxed(),
        ))
        .await;
    let env = environment(&harness);
    let past_hold = HoldHandle::new();
    let past = harness
        .begin(request(
            OperationKind::RemoveInstance,
            OperationResource::Environment(env),
            ScriptedWork::new()
                .step("swap")
                .irreversible()
                .hold(&past_hold)
                .boxed(),
        ))
        .await;
    parked_hold.started().await;
    stopped_hold.started().await;
    past_hold.started().await;

    let release = past_hold.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        release.release();
    });
    let started = Instant::now();
    let report = harness.engine.shutdown(Duration::from_secs(2)).await;

    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(report.parked, 1);
    assert_eq!(report.left_running, 0);
    assert_eq!(
        harness
            .engine
            .begin(manager_request(
                OperationKind::UpdateManager,
                ScriptedWork::new().step("swap").boxed()
            ))
            .await,
        Err(BeginError::ShuttingDown)
    );
    let queued = harness.registry.operation(parked).unwrap().value.unwrap();
    assert_eq!(queued.state(), OperationState::Queued);
    let failed = harness.registry.operation(stopped).unwrap().value.unwrap();
    assert_eq!(failed.state(), OperationState::Failed);
    assert_eq!(
        failed.failure().map(|f| (f.code().as_str(), f.retryable())),
        Some(("daemon_stopping", true))
    );
    assert!(
        std::fs::read_dir(harness.layout.layout().instances_dir())
            .unwrap()
            .next()
            .is_none(),
        "the stopped operation was compensated"
    );
    assert_eq!(
        harness
            .registry
            .operation(past)
            .unwrap()
            .value
            .unwrap()
            .state(),
        OperationState::Succeeded
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovery_deletes_only_the_scratch_folders_it_may() {
    let harness = Harness::new().await;
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").boxed(),
        ))
        .await;
    wait_state(&harness.engine, id, OperationState::Succeeded).await;
    let operations = harness.layout.layout().operations_dir();
    std::fs::create_dir(operations.join(id.to_string())).unwrap();
    std::fs::create_dir(operations.join("notes")).unwrap();

    let (engine, _) = harness.restarted(fast());
    let report = engine.recover().await.unwrap();

    assert_eq!((report.scratch_removed, report.scratch_leftover), (1, 1));
    assert!(!operations.join(id.to_string()).exists());
    assert!(operations.join("notes").is_dir());
}

/// Makes an instance folder in a reversible step, then waits for `hold`
/// without looking at cancels, as a work busy in a long call would, and
/// only then reaches its checkpoint.
struct SlowToStop {
    hold: HoldHandle,
}

impl OperationWork for SlowToStop {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "stage".parse().expect("stage is a stage name"),
            ))
            .await?;
            let (id, created) = ctx.new_instance_id().await?;
            let root = ctx.layout().layout().instance_paths(id).root;
            std::fs::create_dir_all(&root).expect("the test instance folder can be made");
            ctx.confirm_created(created).await?;
            self.hold.mark_started();
            self.hold.released().await;
            ctx.check_cancel()?;
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_lets_a_cancel_requested_operation_finish_its_compensation_within_grace() {
    let harness = Harness::new().await;
    let hold = HoldHandle::new();
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(SlowToStop { hold: hold.clone() }),
        ))
        .await;
    hold.started().await;
    assert_eq!(
        harness.engine.cancel(id).await.unwrap().disposition,
        CancelDisposition::Requested
    );
    let release = hold.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        release.release();
    });

    let started = Instant::now();
    let report = harness.engine.shutdown(Duration::from_secs(2)).await;

    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "shutdown waited for the compensation"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(
        (report.parked, report.stopped, report.left_running),
        (0, 0, 0)
    );
    let closed = harness.registry.operation(id).unwrap().value.unwrap();
    assert_eq!(closed.state(), OperationState::Cancelled);
    let journal = harness
        .registry
        .operation_journal(id)
        .unwrap()
        .value
        .unwrap();
    assert_eq!(journal.created[0].state(), CreatedState::Compensated);
    assert!(
        std::fs::read_dir(harness.layout.layout().instances_dir())
            .unwrap()
            .next()
            .is_none(),
        "the compensation removed the folder"
    );
}

/// Records a custom item in a repeatable step that finishes, then fails
/// after the step: no step is `failed`, so only the compensation that
/// starts marks the operation as one that never resumes.
struct FailsAfterItsStep;

impl OperationWork for FailsAfterItsStep {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            let step = StepSpec::new("download".parse().expect("download is a stage name"));
            ctx.begin_step(step.repeatable()).await?;
            let created = ctx
                .record_custom_intent(
                    "test_stall"
                        .parse()
                        .expect("test_stall is a compensator name"),
                    "partial".parse().expect("partial is an item key"),
                    None,
                )
                .await?;
            ctx.confirm_created(created).await?;
            ctx.finish_step(None).await?;
            Err(WorkError::Failed(TestError::boxed(
                "runtime.extract_failed",
                true,
            )))
        })
    }
}

/// A compensator that never ends until its hold is released: the daemon
/// dies in the middle of compensation.
struct Stalls(HoldHandle);

impl Compensator for Stalls {
    fn compensate(
        &self,
        _scope: CompensationScope,
        _record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>> {
        let hold = self.0.clone();
        Box::pin(async move {
            hold.mark_started();
            hold.released().await;
            Ok(CompensationResult::Compensated)
        })
    }
}

/// A compensator that counts what it undid.
struct Counts(Arc<AtomicU32>);

impl Compensator for Counts {
    fn compensate(
        &self,
        _scope: CompensationScope,
        _record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(CompensationResult::Compensated) })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crash_in_the_middle_of_compensation_is_compensated_again_and_never_resumed() {
    let harness = Harness::new().await;
    let stall = HoldHandle::new();
    harness.engine.register_compensator(
        "test_stall".parse().unwrap(),
        Arc::new(Stalls(stall.clone())),
    );
    let mut begin = request(
        OperationKind::InstallRuntime,
        OperationResource::Runtime(RuntimeId::generate()),
        Box::new(FailsAfterItsStep),
    );
    begin.creates_resource = true;
    let id = harness.begin(begin).await;
    stall.started().await;

    let (engine, _) = harness.restarted(fast());
    let effects = EffectCounter::new();
    engine.register_factory(
        OperationKind::InstallRuntime,
        Arc::new(ScriptedFactory::always(
            ScriptedWork::new()
                .step("download")
                .repeatable()
                .effects(&effects),
        )),
    );
    let undone = Arc::new(AtomicU32::new(0));
    engine.register_compensator(
        "test_stall".parse().unwrap(),
        Arc::new(Counts(Arc::clone(&undone))),
    );
    let report = engine.recover().await.unwrap();

    assert_eq!((report.resumed, report.auto_compensating), (0, 1));
    let interrupted = engine.get(id).await.unwrap().unwrap();
    assert!(interrupted.view.journal.resumable());
    assert!(interrupted.view.journal.compensation_started_at().is_some());
    assert!(
        interrupted
            .steps
            .iter()
            .all(|step| step.state() == StepState::Done),
        "no step failed; the started compensation alone keeps it from resuming"
    );
    assert_eq!(
        interrupted.view.operation.recovery_hint(),
        Some(RecoveryHint::AutoCompensate)
    );
    engine.start();
    let closed = wait_state(&engine, id, OperationState::Failed).await;
    assert_eq!(
        closed
            .view
            .operation
            .failure()
            .map(|f| (f.code().as_str(), f.retryable())),
        Some(("interrupted", true))
    );
    assert_eq!(closed.created[0].state(), CreatedState::Compensated);
    assert_eq!(undone.load(Ordering::SeqCst), 1);
    assert_eq!(effects.get(), 0, "the work never ran again");
}

/// Always fails to undo its item.
struct NeverUndoes;

impl Compensator for NeverUndoes {
    fn compensate(
        &self,
        _scope: CompensationScope,
        _record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>> {
        Box::pin(async {
            Err(CompensationError::new(
                "marker_busy".parse().expect("a reason code"),
                "the marker is busy",
            ))
        })
    }
}

/// Records one custom item for [`NeverUndoes`], then fails before its
/// point of no return.
struct LeavesAnItem;

impl OperationWork for LeavesAnItem {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("download".parse().expect("a step name")).repeatable())
                .await?;
            let item = ctx
                .record_custom_intent(
                    "never_undoes".parse().expect("a compensator name"),
                    "partial".parse().expect("an item key"),
                    None,
                )
                .await?;
            ctx.confirm_created(item).await?;
            Err(WorkError::Failed(TestError::boxed(
                "runtime.checksum_mismatch",
                true,
            )))
        })
    }
}

async fn resume_reason(harness: &Harness, id: stackly_domain::OperationId) -> NotResumableReason {
    match harness.engine.resume(id, None).await {
        Err(EngineError::NotResumable { reason, .. }) => reason,
        other => panic!("expected a refused resume, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_names_why_it_refuses_a_step_a_factory_or_a_failed_compensation() {
    // Spec 0007 AC-11: operation.not_resumable carries its reason.
    let harness = Harness::new().await;
    harness
        .engine
        .register_compensator("never_undoes".parse().unwrap(), Arc::new(NeverUndoes));
    harness.engine.register_factory(
        OperationKind::InstallRuntime,
        Arc::new(ScriptedFactory::always(ScriptedWork::new().step("fetch"))),
    );
    let once = harness
        .begin(install(
            ScriptedWork::new()
                .step("fetch")
                .irreversible()
                .fail("runtime.checksum_mismatch", true),
        ))
        .await;
    let no_factory = harness
        .begin({
            let mut begin = request(
                OperationKind::BackupInstance,
                OperationResource::Runtime(RuntimeId::generate()),
                ScriptedWork::new()
                    .step("copy")
                    .repeatable()
                    .irreversible()
                    .fail("runtime.checksum_mismatch", true)
                    .boxed(),
            );
            begin.creates_resource = true;
            begin
        })
        .await;
    let compensation = harness
        .begin({
            let mut begin = request(
                OperationKind::InstallRuntime,
                OperationResource::Runtime(RuntimeId::generate()),
                Box::new(LeavesAnItem),
            );
            begin.creates_resource = true;
            begin
        })
        .await;
    for id in [once, no_factory, compensation] {
        wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;
    }

    assert_eq!(
        resume_reason(&harness, once).await,
        NotResumableReason::StepNotRepeatable
    );
    assert_eq!(
        resume_reason(&harness, no_factory).await,
        NotResumableReason::NoFactory
    );
    assert_eq!(
        resume_reason(&harness, compensation).await,
        NotResumableReason::CompensationFailed
    );
}

/// Records a custom item for the stalling compensator in a reversible
/// step, then waits there.
struct ItemThenHold(HoldHandle);

impl OperationWork for ItemThenHold {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("stage".parse().expect("a step name")))
                .await?;
            let item = ctx
                .record_custom_intent(
                    "test_stall".parse().expect("a compensator name"),
                    "staged".parse().expect("an item key"),
                    None,
                )
                .await?;
            ctx.confirm_created(item).await?;
            self.0.mark_started();
            self.0.released().await;
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn while_automatic_compensation_runs_resume_and_abandon_refuse_with_auto_compensating() {
    // Spec 0007 AC-11: abandon during automatic compensation gets
    // not_abandonable with auto_compensating, and so does resume.
    let harness = Harness::new().await;
    let held = HoldHandle::new();
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(ItemThenHold(held.clone())),
        ))
        .await;
    held.started().await;

    let (engine, _) = harness.restarted(fast());
    let stall = HoldHandle::new();
    engine.register_compensator(
        "test_stall".parse().unwrap(),
        Arc::new(Stalls(stall.clone())),
    );
    engine.recover().await.unwrap();
    engine.start();
    stall.started().await;
    let running = engine.get(id).await.unwrap().unwrap();
    let resumed = engine.resume(id, None).await;
    let abandoned = engine.abandon(id, running.view.operation.revision()).await;
    stall.release();
    let closed = wait_state(&engine, id, OperationState::Failed).await;

    assert_eq!(
        running.view.operation.recovery_hint(),
        Some(RecoveryHint::AutoCompensate)
    );
    assert!(running.view.auto_compensating);
    assert!(running.view.recovery_actions.is_empty());
    assert!(matches!(
        resumed,
        Err(EngineError::NotResumable {
            reason: NotResumableReason::AutoCompensating,
            ..
        })
    ));
    assert!(matches!(
        abandoned,
        Err(EngineError::NotAbandonable {
            reason: NotAbandonableReason::AutoCompensating,
            ..
        })
    ));
    assert_eq!(
        closed.view.operation.failure().map(|f| f.code().as_str()),
        Some("interrupted")
    );
}
