//! The operation hooks of feature 11 (spec 0007, spec 0011): recording the
//! intent to create a secret reference after a probe, and compensating it
//! through the `secret_ref` compensator.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use stackly_core::operation::testing::RecordingEvents;
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BoxFuture, BusyPolicy, EngineDeps, EngineLimits, OperationContext,
    OperationDetailView, OperationEngine, OperationError, OperationOutcome, OperationWork, Origin,
    Redact, StepSpec, WorkError, WorkOutcome, WorkResult,
};
use stackly_core::prepare_layout;
use stackly_core::secret::{
    PutRequest, PutTarget, SECRET_REF_COMPENSATOR, SecretCaller, SecretInput, SecretRefCompensator,
};
use stackly_domain::{
    CreatedItem, CreatedState, DetailKey, ErrorCodeName, InstanceId, JournalPayload, OperationId,
    OperationKind, OperationResource, OperationState, Registry, SecretOwner, SecretRef,
};
use stackly_platform::WriteMode;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::{Harness, instance_reference, reference};

/// What the work does once it recorded the intent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Then {
    /// Stores a generated value, confirms the item, and fails a later step.
    StoreAndFail,
    /// Finishes.
    Succeed,
}

/// Makes an instance ID, records the intent for a reference the test
/// derives from it, and goes on as `then` says.
struct Intent {
    reference_of: fn(InstanceId) -> SecretRef,
    then: Then,
    /// Put an item in the store for the reference just before the intent.
    plant: bool,
    harness: Arc<Harness>,
    made: Arc<Mutex<Option<SecretRef>>>,
}

impl OperationWork for Intent {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("secrets".parse().expect("a step name")))
                .await?;
            let (id, tree) = ctx.new_instance_id().await?;
            // No folder is made, so the tree is never compensated.
            ctx.not_created(tree, "already_exists".parse().expect("a reason code"))
                .await?;
            let target = (self.reference_of)(id);
            *self.made.lock().expect("the test lock is not poisoned") = Some(target.clone());
            if self.plant {
                self.harness.store().insert(&target, "an-item-left-before");
            }
            let handle = ctx.record_secret_intent(target.clone()).await?;
            if self.then == Then::Succeed {
                // An item still intended at the end is a bug, so confirm it.
                ctx.confirm_created(handle).await?;
                ctx.finish_step(None).await?;
                return Ok(WorkOutcome {
                    result: OperationOutcome::default(),
                    finalize: None,
                });
            }
            let service = Arc::clone(&self.harness.service);
            let operation = ctx.id();
            tokio::task::spawn_blocking(move || {
                service.put(
                    PutRequest::new(
                        PutTarget::New {
                            reference: target,
                            owner: SecretOwner::Instance(id),
                            purpose: "admin-password".parse().expect("a purpose"),
                            field: None,
                        },
                        SecretInput::Generate,
                        WriteMode::Create,
                    ),
                    &SecretCaller::background().in_operation(operation),
                )
            })
            .await
            .expect("the put task finishes")
            .map_err(|error| WorkError::Failed(Box::new(error)))?;
            ctx.confirm_created(handle).await?;
            ctx.finish_step(None).await?;
            ctx.begin_step(StepSpec::new("initdb".parse().expect("a step name")))
                .await?;
            Err(WorkError::Failed(Box::new(InitFailed)))
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("initdb failed")]
struct InitFailed;

impl OperationError for InitFailed {
    fn code(&self) -> ErrorCodeName {
        "instance.init_failed".parse().expect("an error code")
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        Vec::new()
    }
}

struct Engine {
    harness: Arc<Harness>,
    engine: OperationEngine,
}

impl Engine {
    async fn new() -> Self {
        let harness = Arc::new(Harness::new());
        let layout =
            Arc::new(prepare_layout(&harness.platform).expect("a fresh test root prepares"));
        let engine = OperationEngine::new(
            EngineDeps {
                registry: Arc::clone(&harness.registry) as Arc<dyn Registry>,
                platform: Arc::clone(&harness.platform),
                layout,
                events: Arc::new(RecordingEvents::new()),
                redactor: Arc::clone(&harness.redactor) as Arc<dyn Redact>,
            },
            EngineLimits::default()
                .with_intervals(Duration::from_millis(100), Duration::from_millis(20)),
        );
        engine.recover().await.expect("an empty registry recovers");
        engine.start();
        Self { harness, engine }
    }

    async fn run(
        &self,
        reference_of: fn(InstanceId) -> SecretRef,
        then: Then,
    ) -> (OperationDetailView, SecretRef) {
        self.run_planted(reference_of, then, false).await
    }

    async fn run_planted(
        &self,
        reference_of: fn(InstanceId) -> SecretRef,
        then: Then,
        plant: bool,
    ) -> (OperationDetailView, SecretRef) {
        let made = Arc::new(Mutex::new(None));
        let work = Intent {
            reference_of,
            then,
            plant,
            harness: Arc::clone(&self.harness),
            made: Arc::clone(&made),
        };
        let request = BeginRequest {
            kind: OperationKind::CreateInstance,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: Vec::new(),
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work: Box::new(work),
            precheck: None,
        };
        let id = match self.engine.begin(request).await {
            Ok(BeginOutcome::Created(view)) => view.operation.id(),
            other => panic!("expected a created operation, got {other:?}"),
        };
        let detail = self.settled(id).await;
        let target = made
            .lock()
            .expect("the test lock is not poisoned")
            .clone()
            .expect("the work made a reference");
        (detail, target)
    }

    async fn settled(&self, id: OperationId) -> OperationDetailView {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(detail) = self.engine.get(id).await.expect("the registry reads")
                && (detail.view.operation.state().is_final()
                    || detail.view.operation.state() == OperationState::NeedsRecovery)
            {
                return detail;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "operation {id} did not settle"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

fn admin(id: InstanceId) -> SecretRef {
    instance_reference(id, "admin-password")
}

fn failure_code(detail: &OperationDetailView) -> Option<String> {
    detail
        .view
        .journal
        .failure_error()
        .map(|error| error.code().as_str().to_owned())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_secret_intent_is_recorded_after_a_probe_finds_no_item() {
    let engine = Engine::new().await;

    let (detail, target) = engine.run(admin, Then::Succeed).await;

    assert_eq!(detail.view.operation.state(), OperationState::Succeeded);
    let secret: Vec<_> = detail
        .created
        .iter()
        .filter(|created| created.item() == &CreatedItem::SecretRef(target.clone()))
        .collect();
    assert_eq!(secret.len(), 1);
    assert_eq!(
        engine
            .harness
            .store()
            .calls()
            .iter()
            .filter(|call| call.reference == target)
            .count(),
        1,
        "one probe, nothing else"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_existing_item_fails_the_intent_with_secret_conflict_and_records_nothing() {
    let engine = Engine::new().await;

    let (detail, target) = engine.run_planted(admin, Then::Succeed, true).await;

    assert_eq!(detail.view.operation.state(), OperationState::Failed);
    assert_eq!(failure_code(&detail).as_deref(), Some("secret.conflict"));
    assert!(
        detail
            .created
            .iter()
            .all(|created| created.item() != &CreatedItem::SecretRef(target.clone()))
    );
    assert_eq!(
        engine.harness.store().stored_value(&target).as_deref(),
        Some("an-item-left-before"),
        "the item that was there stays"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reference_of_another_instance_is_refused_before_the_probe() {
    fn elsewhere(_: InstanceId) -> SecretRef {
        reference("local:instance/0199f000-0000-7000-8000-0000000000ff/admin-password")
    }
    let engine = Engine::new().await;

    let (detail, target) = engine.run(elsewhere, Then::Succeed).await;

    assert_eq!(detail.view.operation.state(), OperationState::Failed);
    assert!(
        detail
            .created
            .iter()
            .all(|created| !matches!(created.item(), CreatedItem::SecretRef(_)))
    );
    assert!(
        engine
            .harness
            .store()
            .calls()
            .iter()
            .all(|call| call.reference != target),
        "the store is never asked about another instance's reference"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_operation_removes_its_secret_through_the_secret_ref_compensator() {
    let ev = evidence!(
        covers = ["BR-023", "REQ-016"],
        kind = Kind::Fixture,
        expected = "an operation that recorded a secret intent, stored a generated value, and \
                    then failed has its reference row and store item removed by the secret_ref \
                    compensator, and its created record ends compensated",
    );
    let engine = Engine::new().await;
    engine.engine.register_compensator(
        SECRET_REF_COMPENSATOR,
        Arc::new(SecretRefCompensator::new(Arc::clone(
            &engine.harness.service,
        ))),
    );

    let (detail, target) = engine.run(admin, Then::StoreAndFail).await;

    assert_eq!(detail.view.operation.state(), OperationState::Failed);
    assert_eq!(
        failure_code(&detail).as_deref(),
        Some("instance.init_failed")
    );
    let record = detail
        .created
        .iter()
        .find(|created| created.item() == &CreatedItem::SecretRef(target.clone()))
        .expect("the secret intent is recorded");
    assert_eq!(record.state(), CreatedState::Compensated);
    assert!(engine.harness.view(&target).is_none());
    assert!(!engine.harness.store().contains(&target));
    let delete = engine
        .harness
        .audit()
        .into_iter()
        .rev()
        .find(|row| row.action.as_str() == "secret.delete")
        .expect("the compensation deletes through the use case");
    assert!(delete.operation.is_some());
    ev.pass();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_the_secret_ref_compensator_the_compensation_fails() {
    let engine = Engine::new().await;

    let (detail, target) = engine.run(admin, Then::StoreAndFail).await;

    assert_eq!(detail.view.operation.state(), OperationState::NeedsRecovery);
    let record = detail
        .created
        .iter()
        .find(|created| created.item() == &CreatedItem::SecretRef(target.clone()))
        .expect("the secret intent is recorded");
    assert_eq!(record.state(), CreatedState::CompensationFailed);
    assert_eq!(
        record.detail().map(|detail| detail.as_str()),
        Some("compensator_missing")
    );
    assert!(engine.harness.store().contains(&target));
}
