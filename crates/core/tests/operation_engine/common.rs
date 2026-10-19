//! The harness every engine test shares.

use std::sync::Arc;
use std::time::Duration;

use jiff::SignedDuration;
use stackly_core::operation::testing::RecordingEvents;
use stackly_core::operation::{
    BeginOutcome, BeginRequest, BusyPolicy, EngineDeps, EngineLimits, NoSecretsYet,
    OperationDetailView, OperationEngine, OperationWork, Origin, RequestOrigin,
};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{
    JournalPayload, OperationId, OperationKind, OperationResource, OperationState,
};
use stackly_platform::{Clock, Platform};
use stackly_testkit::{FakePlatform, FakeRegistry, TestRoot};

/// Limits with short intervals, so retries and progress windows take
/// milliseconds.
pub fn fast() -> EngineLimits {
    EngineLimits::default().with_intervals(Duration::from_millis(100), Duration::from_millis(20))
}

/// One engine on fakes in its own test root.
pub struct Harness {
    pub root: TestRoot,
    pub fake: FakePlatform,
    pub platform: Arc<Platform>,
    pub layout: Arc<PreparedLayout>,
    pub registry: Arc<FakeRegistry>,
    pub events: RecordingEvents,
    pub engine: OperationEngine,
}

impl Harness {
    /// A recovered and started engine with [`fast`] limits.
    pub async fn new() -> Self {
        Self::with_limits(fast()).await
    }

    /// A recovered and started engine with `limits`.
    pub async fn with_limits(limits: EngineLimits) -> Self {
        let harness = Self::idle(limits);
        harness
            .engine
            .recover()
            .await
            .expect("an empty registry recovers");
        harness.engine.start();
        harness
    }

    /// An engine that has not recovered yet.
    pub fn idle(limits: EngineLimits) -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakePlatform::new(&root);
        let platform = Arc::new(fake.platform());
        let layout = Arc::new(prepare_layout(&platform).expect("a fresh test root prepares"));
        let registry = Arc::new(FakeRegistry::new(
            Arc::new(fake.clock().clone()),
            fake.clock().now(),
        ));
        let events = RecordingEvents::new();
        let engine = engine_on(&platform, &layout, &registry, &events, limits);
        Self {
            root,
            fake,
            platform,
            layout,
            registry,
            events,
            engine,
        }
    }

    /// A second engine on the same registry and folders, as the daemon
    /// after a crash: not recovered yet. The first engine's tasks are left
    /// as they are.
    pub fn restarted(&self, limits: EngineLimits) -> (OperationEngine, RecordingEvents) {
        let events = RecordingEvents::new();
        let engine = engine_on(
            &self.platform,
            &self.layout,
            &self.registry,
            &events,
            limits,
        );
        (engine, events)
    }

    /// Moves the fake clock one second on, so the next operation is newer.
    pub fn tick(&self) {
        self.fake.clock().advance(SignedDuration::from_secs(1));
    }

    /// Begins `request` and returns the new operation's ID.
    pub async fn begin(&self, request: BeginRequest) -> OperationId {
        self.tick();
        match self.engine.begin(request).await {
            Ok(BeginOutcome::Created(view)) => view.operation.id(),
            other => panic!("expected a created operation, got {other:?}"),
        }
    }
}

fn engine_on(
    platform: &Arc<Platform>,
    layout: &Arc<PreparedLayout>,
    registry: &Arc<FakeRegistry>,
    events: &RecordingEvents,
    limits: EngineLimits,
) -> OperationEngine {
    OperationEngine::new(
        EngineDeps {
            registry: Arc::clone(registry) as Arc<dyn stackly_domain::Registry>,
            platform: Arc::clone(platform),
            layout: Arc::clone(layout),
            events: Arc::new(events.clone()),
            redactor: Arc::new(NoSecretsYet),
        },
        limits,
    )
}

/// A background request for `work` on `resource`.
pub fn request(
    kind: OperationKind,
    resource: OperationResource,
    work: Box<dyn OperationWork>,
) -> BeginRequest {
    BeginRequest {
        kind,
        resource,
        creates_resource: false,
        extra_locks: Vec::new(),
        busy: BusyPolicy::Queue,
        expected_revision: None,
        origin: Origin::Background,
        input: JournalPayload::empty(),
        work,
    }
}

/// A request on the manager, which always exists.
pub fn manager_request(kind: OperationKind, work: Box<dyn OperationWork>) -> BeginRequest {
    request(kind, OperationResource::Manager, work)
}

/// A client origin with `key` and a fingerprint made of `byte`.
pub fn user(key: &str, byte: u8) -> Origin {
    Origin::Request(RequestOrigin {
        method: "manager.update".parse().expect("a method name"),
        key: key.parse().expect("an idempotency key"),
        fingerprint: format!("{byte:02x}")
            .repeat(32)
            .parse()
            .expect("a 64 hex digest"),
        client: "cli".parse().expect("a client code"),
        interactive: true,
    })
}

/// Waits until the operation matches `ready`, for at most ten seconds.
pub async fn wait_until(
    engine: &OperationEngine,
    id: OperationId,
    ready: impl Fn(&OperationDetailView) -> bool,
) -> OperationDetailView {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(detail) = engine.get(id).await.expect("the registry reads")
            && ready(&detail)
        {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "operation {id} did not get there in time: {:?}",
            engine.get(id).await
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Waits until the operation is in `state`.
pub async fn wait_state(
    engine: &OperationEngine,
    id: OperationId,
    state: OperationState,
) -> OperationDetailView {
    wait_until(engine, id, |detail| detail.view.operation.state() == state).await
}
