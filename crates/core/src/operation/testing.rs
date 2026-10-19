//! Test helpers for the operation engine (spec 0007, "`ScriptedWork`"):
//! scripted works, their factory, a side effect counter, holds a test
//! releases, an event recorder, and the compensator conformance check.
//! Not part of the engine's API.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use stackly_domain::{
    CreatedRecord, DetailKey, ErrorCodeName, JournalView, Operation, OperationId, OperationState,
    Progress, ResourceRevision, StageName,
};
use tokio::sync::watch;

use super::compensation::{CompensationResult, CompensationScope, Compensator};
use super::context::OperationContext;
use super::work::{
    BoxFuture, OperationError, OperationOutcome, OperationWork, RebuildError, StepSpec, StepStart,
    WorkError, WorkFactory, WorkOutcome, WorkResult,
};
use super::{OperationEvents, OperationView};

/// Counts side effects shared between works and the test.
#[derive(Debug, Clone, Default)]
pub struct EffectCounter(Arc<AtomicU64>);

impl EffectCounter {
    /// A counter at 0.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one effect.
    pub fn add(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    /// Returns how many effects happened.
    #[must_use]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Holds a scripted step until the test releases it, and tells the test
/// when the step began.
#[derive(Debug, Clone)]
pub struct HoldHandle {
    started: watch::Sender<bool>,
    released: watch::Sender<bool>,
}

impl Default for HoldHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl HoldHandle {
    /// A hold that is not started and not released.
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: watch::channel(false).0,
            released: watch::channel(false).0,
        }
    }

    /// Waits until the held step began.
    pub async fn started(&self) {
        let mut started = self.started.subscribe();
        let _ = started.wait_for(|started| *started).await;
    }

    /// Lets the held step go on.
    pub fn release(&self) {
        self.released.send_replace(true);
    }

    /// Tells the test the held step began; scripted works call it, and
    /// hand written works may too.
    pub fn mark_started(&self) {
        self.started.send_replace(true);
    }

    /// Waits until the test released the hold.
    pub async fn released(&self) {
        let mut released = self.released.subscribe();
        let _ = released.wait_for(|released| *released).await;
    }
}

/// What a scripted step does after it began.
#[derive(Debug, Clone)]
enum Behavior {
    Succeed,
    Fail {
        code: ErrorCodeName,
        retryable: bool,
    },
    Panic,
}

#[derive(Debug, Clone)]
struct ScriptedStep {
    spec: StepSpec,
    behavior: Behavior,
    checks: u32,
    progress: Option<(u64, u64)>,
    creates_instance_tree: bool,
    hold: Option<HoldHandle>,
}

/// The error a failing scripted step returns.
#[derive(Debug, Clone, thiserror::Error)]
#[error("scripted step failed with {code}")]
pub struct ScriptedError {
    code: ErrorCodeName,
    retryable: bool,
}

impl OperationError for ScriptedError {
    fn code(&self) -> ErrorCodeName {
        self.code.clone()
    }

    fn retryable(&self) -> bool {
        self.retryable
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        Vec::new()
    }
}

/// A work built from scripted steps. Each `step` adds a reversible step
/// that is not repeatable and succeeds; the modifiers that follow apply to
/// the last step added. The work adds one effect to its counter when every
/// step is done.
#[derive(Debug, Clone, Default)]
pub struct ScriptedWork {
    steps: Vec<ScriptedStep>,
    effects: EffectCounter,
    result: OperationOutcome,
}

impl ScriptedWork {
    /// A work with no steps.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn last(&mut self) -> &mut ScriptedStep {
        self.steps
            .last_mut()
            .expect("a step modifier follows a step")
    }

    /// Adds a step named `name`.
    ///
    /// # Panics
    ///
    /// Panics when `name` is not a stage name.
    #[must_use]
    pub fn step(mut self, name: &str) -> Self {
        let name: StageName = name.parse().expect("a scripted step has a stage name");
        self.steps.push(ScriptedStep {
            spec: StepSpec::new(name),
            behavior: Behavior::Succeed,
            checks: 0,
            progress: None,
            creates_instance_tree: false,
            hold: None,
        });
        self
    }

    /// Makes the last step repeatable.
    #[must_use]
    pub fn repeatable(mut self) -> Self {
        let step = self.last();
        step.spec = step.spec.clone().repeatable();
        self
    }

    /// Makes the last step one that cannot be undone.
    #[must_use]
    pub fn irreversible(mut self) -> Self {
        let step = self.last();
        step.spec = step.spec.clone().irreversible();
        self
    }

    /// Makes the last step fail with `code`.
    ///
    /// # Panics
    ///
    /// Panics when `code` is not an error code name.
    #[must_use]
    pub fn fail(mut self, code: &str, retryable: bool) -> Self {
        let code = code.parse().expect("a scripted failure has an error code");
        self.last().behavior = Behavior::Fail { code, retryable };
        self
    }

    /// Makes the last step panic.
    #[must_use]
    pub fn panics(mut self) -> Self {
        self.last().behavior = Behavior::Panic;
        self
    }

    /// Holds the last step until `hold` is released, or the step must stop.
    #[must_use]
    pub fn hold(mut self, hold: &HoldHandle) -> Self {
        self.last().hold = Some(hold.clone());
        self
    }

    /// Makes the last step call `check_cancel` `n` times.
    #[must_use]
    pub fn check_cancel_every(mut self, n: u32) -> Self {
        self.last().checks = n;
        self
    }

    /// Makes the last step create a new instance folder through
    /// `new_instance_id` and confirm it.
    #[must_use]
    pub fn creates_instance_tree(mut self) -> Self {
        self.last().creates_instance_tree = true;
        self
    }

    /// Makes the last step report counted progress.
    #[must_use]
    pub fn progress(mut self, done: u64, total: u64) -> Self {
        self.last().progress = Some((done, total));
        self
    }

    /// Counts the work's side effect in `effects`.
    #[must_use]
    pub fn effects(mut self, effects: &EffectCounter) -> Self {
        self.effects = effects.clone();
        self
    }

    /// Sets the result the work reports.
    #[must_use]
    pub fn result(mut self, result: OperationOutcome) -> Self {
        self.result = result;
        self
    }

    /// Returns the work boxed for a `BeginRequest`.
    #[must_use]
    pub fn boxed(self) -> Box<dyn OperationWork> {
        Box::new(self)
    }
}

async fn run_step(ctx: &mut OperationContext, step: ScriptedStep) -> Result<(), WorkError> {
    if let StepStart::AlreadyDone { .. } = ctx.begin_step(step.spec.clone()).await? {
        return Ok(());
    }
    if step.creates_instance_tree {
        let (id, created) = ctx.new_instance_id().await?;
        let root = ctx.layout().layout().instance_paths(id).root;
        let platform = std::sync::Arc::clone(ctx.platform());
        let made = tokio::task::spawn_blocking(move || {
            platform.fs().create_dir_private(&root)?;
            platform.fs().create_dir_private(&root.join("data"))?;
            platform
                .fs()
                .write_file_atomic(&root.join("data").join("PG_VERSION"), b"17")
        })
        .await
        .map_err(|_| {
            WorkError::Engine(super::EngineError::Internal {
                reason: "a scripted folder task stopped",
            })
        })?;
        made.map_err(|error| WorkError::Engine(super::EngineError::Platform(error)))?;
        ctx.confirm_created(created).await?;
    }
    if let Some((done, total)) = step.progress {
        ctx.set_progress(Progress::Counted { done, total });
    }
    if let Some(hold) = &step.hold {
        hold.mark_started();
        tokio::select! {
            () = hold.released() => {}
            () = ctx.stopped() => {}
        }
    }
    for _ in 0..step.checks.max(u32::from(step.hold.is_some())) {
        ctx.check_cancel()?;
        tokio::task::yield_now().await;
    }
    match step.behavior {
        Behavior::Succeed => {}
        Behavior::Fail { code, retryable } => {
            return Err(WorkError::Failed(Box::new(ScriptedError {
                code,
                retryable,
            })));
        }
        Behavior::Panic => panic!("the scripted step panics on purpose"),
    }
    ctx.finish_step(None).await
}

impl OperationWork for ScriptedWork {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            let Self {
                steps,
                effects,
                result,
            } = *self;
            for step in steps {
                run_step(&mut ctx, step).await?;
            }
            effects.add();
            Ok(WorkOutcome {
                result,
                finalize: None,
            })
        })
    }
}

/// How a [`ScriptedFactory`] builds a work.
type BuildWork =
    Box<dyn Fn(&Operation, &JournalView) -> Result<ScriptedWork, RebuildError> + Send + Sync>;

/// Rebuilds scripted works, for resume and for queued operations after a
/// restart.
pub struct ScriptedFactory {
    build: BuildWork,
}

impl fmt::Debug for ScriptedFactory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptedFactory").finish_non_exhaustive()
    }
}

impl ScriptedFactory {
    /// A factory that builds each work with `build`.
    pub fn new(
        build: impl Fn(&Operation, &JournalView) -> Result<ScriptedWork, RebuildError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            build: Box::new(build),
        }
    }

    /// A factory that always builds `work` again.
    #[must_use]
    pub fn always(work: ScriptedWork) -> Self {
        Self::new(move |_, _| Ok(work.clone()))
    }
}

impl WorkFactory for ScriptedFactory {
    fn rebuild(
        &self,
        operation: &Operation,
        journal: &JournalView,
    ) -> Result<Box<dyn OperationWork>, RebuildError> {
        (self.build)(operation, journal).map(ScriptedWork::boxed)
    }
}

/// One event the recorder saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedEvent {
    /// `state_changed` or `progress`.
    pub kind: &'static str,
    /// The operation.
    pub operation: OperationId,
    /// Its state.
    pub state: OperationState,
    /// Its revision.
    pub revision: ResourceRevision,
}

/// Records every event the engine reports.
#[derive(Debug, Clone, Default)]
pub struct RecordingEvents {
    events: Arc<Mutex<Vec<(RecordedEvent, OperationView)>>>,
}

impl RecordingEvents {
    /// An empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns every event so far, in order.
    #[must_use]
    pub fn events(&self) -> Vec<RecordedEvent> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(event, _)| event.clone())
            .collect()
    }

    /// Returns the events of one operation, in order.
    #[must_use]
    pub fn of(&self, operation: OperationId) -> Vec<RecordedEvent> {
        self.events()
            .into_iter()
            .filter(|event| event.operation == operation)
            .collect()
    }

    /// Returns the views of one operation's events, in order.
    #[must_use]
    pub fn views(&self, operation: OperationId) -> Vec<OperationView> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(event, _)| event.operation == operation)
            .map(|(_, view)| view.clone())
            .collect()
    }

    fn push(&self, kind: &'static str, view: &OperationView) {
        let event = RecordedEvent {
            kind,
            operation: view.operation.id(),
            state: view.operation.state(),
            revision: view.operation.revision(),
        };
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((event, view.clone()));
    }
}

impl OperationEvents for RecordingEvents {
    fn state_changed(&self, view: &OperationView) {
        self.push("state_changed", view);
    }

    fn progress(&self, view: &OperationView) {
        self.push("progress", view);
    }
}

/// Checks a compensator against the rules every compensator keeps: it
/// compensates an item of its operation, gives the same result when it runs
/// again (the item is gone by then), and leaves an item that is not its
/// operation's alone with `NotOwned`.
///
/// # Panics
///
/// Panics with the rule the compensator broke.
pub async fn compensator_conformance(
    compensator: &dyn Compensator,
    scope: CompensationScope,
    owned: &CreatedRecord,
    foreign: &CreatedRecord,
) {
    let first = compensator.compensate(scope.clone(), owned).await;
    assert_eq!(
        first,
        Ok(CompensationResult::Compensated),
        "a compensator undoes an item of its own operation"
    );
    let again = compensator.compensate(scope.clone(), owned).await;
    assert_eq!(
        again,
        Ok(CompensationResult::Compensated),
        "a compensator gives the same result when it runs twice"
    );
    let other = compensator.compensate(scope, foreign).await;
    assert!(
        matches!(other, Ok(CompensationResult::NotOwned { .. })),
        "a compensator leaves an item of another operation alone, got {other:?}"
    );
}
