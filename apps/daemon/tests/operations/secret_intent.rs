//! The `secret_ref` compensator inside the real daemon (spec 0011 build
//! steps 4 and 7, spec 0007 hooks of feature 11): an operation that
//! recorded the intent to create a secret reference, stored a value, and
//! failed has its store item removed by the compensator `stacklyd`
//! registered at start, with no compensator added by the test.

use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use stackly_core::operation::{
    BeginRequest, BoxFuture, BusyPolicy, OperationContext, OperationError, OperationWork, Origin,
    StepSpec, WorkError, WorkResult,
};
use stackly_core::secret::{SecretWorker, WorkerConfig};
use stackly_domain::{
    DetailKey, ErrorCodeName, JournalPayload, OperationKind, OperationResource, SecretRef,
};
use stackly_platform::SecretStore;
use stackly_protocol::ErrorBody;
use stackly_protocol::codes::request;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::operation::{CreatedState, OperationState};
use stackly_testkit::FakeSecretStore;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use stacklyd::dispatch::Handlers;
use stacklyd::ipc::operation::{TestStart, TestStartingMethod};

use crate::support::{METHOD, Raw, Site, accepted, client, key, wait_state};

/// A failure after the secret was stored.
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

/// Makes an instance ID, records the intent for its admin password,
/// stores a value for it in the store, and fails.
struct StoreThenFail {
    store: FakeSecretStore,
    value: String,
    made: Arc<Mutex<Option<SecretRef>>>,
}

impl OperationWork for StoreThenFail {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("secrets".parse().expect("a step name")))
                .await?;
            let (id, tree) = ctx.new_instance_id().await?;
            // No folder is made, so the tree is never compensated.
            ctx.not_created(tree, "already_exists".parse().expect("a reason code"))
                .await?;
            let target = SecretRef::parse(&format!("local:instance/{id}/admin-password"))
                .expect("an instance reference");
            *self.made.lock().unwrap_or_else(PoisonError::into_inner) = Some(target.clone());
            let handle = ctx.record_secret_intent(target.clone()).await?;
            self.store.insert(&target, &self.value);
            ctx.confirm_created(handle).await?;
            ctx.finish_step(None).await?;
            ctx.begin_step(StepSpec::new("initdb".parse().expect("a step name")))
                .await?;
            Err(WorkError::Failed(Box::new(InitFailed)))
        })
    }
}

/// The test's starting method for [`StoreThenFail`].
#[derive(Clone)]
struct Starts {
    store: FakeSecretStore,
    value: String,
    made: Arc<Mutex<Option<SecretRef>>>,
}

impl TestStartingMethod for Starts {
    fn method(&self) -> &'static str {
        METHOD
    }

    fn prepare(&self, params: Option<&Value>) -> Result<TestStart, Box<ErrorBody>> {
        if params.is_none() {
            return Err(Box::new(ErrorBody::new(
                &request::INVALID_PARAMS,
                "params do not match the method",
            )));
        }
        let work = StoreThenFail {
            store: self.store.clone(),
            value: self.value.clone(),
            made: Arc::clone(&self.made),
        };
        Ok(TestStart {
            fingerprint: [7; 32],
            build: Box::new(move |origin: Origin| BeginRequest {
                kind: OperationKind::CreateInstance,
                resource: OperationResource::Manager,
                creates_resource: false,
                extra_locks: Vec::new(),
                busy: BusyPolicy::Queue,
                expected_revision: None,
                origin,
                input: JournalPayload::empty(),
                work: Box::new(work),
                precheck: None,
            }),
        })
    }
}

struct WithStarts(Starts);

impl Handlers for WithStarts {
    fn test_starting_method(&self) -> Option<Arc<dyn TestStartingMethod>> {
        Some(Arc::new(self.0.clone()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_operation_has_its_secret_removed_by_the_compensator_the_daemon_registered() {
    let ev = evidence!(
        covers = ["REQ-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "in a running stacklyd, an operation that recorded a secret intent for an instance reference, stored a value for it, and then failed ends with its secret_ref record compensated and the store item removed, with no compensator registered by the test",
    );
    let store = FakeSecretStore::new();
    let worker = SecretWorker::spawn(Arc::new(store.clone()), WorkerConfig::default())
        .expect("the worker thread starts");
    let mut site = Site::new();
    site.platform = site
        .platform
        .clone()
        .with_secrets(worker as Arc<dyn SecretStore>);
    let credential = FixtureCredential::new();
    let made = Arc::new(Mutex::new(None));
    let starts = Starts {
        store: store.clone(),
        value: credential.value().to_owned(),
        made: Arc::clone(&made),
    };
    let deps = site.deps_with(WithStarts(starts));
    let daemon = stacklyd::server::start(deps)
        .await
        .expect("the server starts");
    daemon.started().await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;

    let frame = raw
        .start(Some(&key()), json!({ "script": "store_then_fail" }))
        .await;
    let started = accepted(&frame);
    let detail = wait_state(&client, started.operation.id, OperationState::Failed).await;
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;

    let target = made
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .expect("the work made a reference");
    let record = detail
        .created
        .iter()
        .find(|created| created.kind == "secret_ref")
        .expect("the secret intent is recorded");
    assert_eq!(record.reference, target.as_str());
    assert_eq!(record.state, CreatedState::Compensated, "{detail:?}");
    assert!(
        !store.contains(&target),
        "the compensator deleted the item the operation stored"
    );
    ev.pass();
}
