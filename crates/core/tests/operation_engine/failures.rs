//! Panics, works that end with items still intended, journal writes that
//! fail, and payloads over their limit (spec 0007 AC-16).

use std::sync::Arc;
use std::time::{Duration, Instant};

use stackly_core::operation::testing::{HoldHandle, ScriptedFactory, ScriptedWork};
use stackly_core::operation::{
    BeginError, BoxFuture, CompensationError, CompensationResult, CompensationScope, Compensator,
    EngineLimits, OperationContext, OperationError, OperationOutcome, OperationWork, StepSpec,
    WorkError, WorkOutcome, WorkResult, codes,
};
use stackly_domain::{
    CreatedRecord, CreatedState, DetailKey, ErrorCodeName, FailureError, FailureItem,
    JournalPayload, LockKey, OperationKind, OperationResource, OperationState, Registry,
    RegistryError, Rule, RuntimeId, StepState,
};
use stackly_platform::SecretValue;
use stackly_testkit::FakeRegistry;

use crate::common::{Harness, TestError, fast, manager_request, request, wait_state};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panicking_work_is_compensated_and_failed_and_the_engine_keeps_running() {
    let harness = Harness::new().await;

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .creates_instance_tree()
                .step("swap")
                .panics()
                .boxed(),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    assert_eq!(
        failed.view.operation.failure().map(|f| f.code().as_str()),
        Some("panicked")
    );
    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("internal.unexpected")
    );
    assert_eq!(failed.created[0].state(), CreatedState::Compensated);
    assert_eq!(failed.steps[1].state(), StepState::Failed);
    let next = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").boxed(),
        ))
        .await;
    wait_state(&harness.engine, next, OperationState::Succeeded).await;
}

/// Records an instance tree and never confirms or declines it.
struct Forgetful;

impl OperationWork for Forgetful {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "create".parse().expect("create step name is valid"),
            ))
            .await?;
            ctx.new_instance_id().await?;
            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_work_that_succeeds_with_an_intended_item_is_compensated_and_failed() {
    let harness = Harness::new().await;

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(Forgetful),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    assert_eq!(
        failed.view.operation.failure().map(|f| f.code().as_str()),
        Some("step_failed")
    );
    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("internal.unexpected")
    );
    assert_eq!(failed.created[0].state(), CreatedState::Compensated);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_journal_write_that_fails_stops_the_work_and_the_final_state_still_lands() {
    let harness = Harness::new().await;
    let hold = HoldHandle::new();
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("stage")
                .creates_instance_tree()
                .hold(&hold)
                .step("swap")
                .boxed(),
        ))
        .await;
    hold.started().await;

    harness.registry.fail_next_commit(RegistryError::NoSpace);
    hold.release();
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    assert_eq!(
        failed
            .view
            .journal
            .failure_error()
            .map(|e| e.code().as_str()),
        Some("internal.unexpected")
    );
    assert_eq!(failed.created[0].state(), CreatedState::Compensated);
    assert!(
        std::fs::read_dir(harness.layout.layout().instances_dir())
            .unwrap()
            .next()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_input_over_its_limit_is_refused_before_anything_is_written() {
    let harness = Harness::new().await;
    let text = format!(r#"{{"blob":"{}"}}"#, "x".repeat(20_000));
    let mut begin = manager_request(
        OperationKind::UpdateManager,
        ScriptedWork::new().step("swap").boxed(),
    );
    begin.input = JournalPayload::new(text, usize::MAX).unwrap();

    let refused = harness.engine.begin(begin).await;

    assert_eq!(
        refused,
        Err(BeginError::PayloadTooLarge {
            field: "input",
            max: 16_384
        })
    );
    assert_eq!(harness.engine.stats().queued, 0);
}

#[test]
fn every_engine_code_is_a_distinct_area_reason_code() {
    let mut seen = std::collections::BTreeSet::new();
    for code in codes::ALL {
        assert!(seen.insert(code.as_str()), "{code} is listed twice");
        assert!(code.as_str().contains('.'));
    }
    assert_eq!(seen.len(), codes::ALL.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_start_commit_frees_the_key_and_slot_and_retries_after_the_interval() {
    let retry = Duration::from_millis(300);
    let harness = Harness::idle(
        EngineLimits::default()
            .with_slots(1, 64)
            .with_intervals(Duration::from_millis(100), retry),
    );
    harness.engine.recover().await.unwrap();
    let unlucky = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("swap").boxed(),
        ))
        .await;
    let mut install = request(
        OperationKind::InstallRuntime,
        OperationResource::Runtime(RuntimeId::generate()),
        ScriptedWork::new().step("download").boxed(),
    );
    install.creates_resource = true;
    let other = harness.begin(install).await;

    harness.registry.fail_next_commit(RegistryError::NoSpace);
    let started = Instant::now();
    harness.engine.start();
    wait_state(&harness.engine, other, OperationState::Succeeded).await;

    let waiting = harness.engine.get(unlucky).await.unwrap().unwrap();
    assert_eq!(waiting.view.operation.state(), OperationState::Queued);
    assert_eq!(waiting.view.operation.started_at(), None);
    assert!(
        started.elapsed() < retry,
        "the other operation took the freed slot before the retry"
    );
    assert_eq!(harness.engine.locks().holder(&LockKey::Manager), None);
    drop(
        harness
            .engine
            .locks()
            .try_lock_internal(&[LockKey::Manager], "probe")
            .expect("the failed start released its key"),
    );
    let done = wait_state(&harness.engine, unlucky, OperationState::Succeeded).await;
    assert!(
        started.elapsed() >= retry,
        "the start ran again only after the retry interval"
    );
    assert_eq!(done.steps.len(), 1);
}

/// A fixture value, never a real secret.
const LEAKED: &str = "stackly-fixture-credential-fedcba9876543210";

/// An error whose message and detail carry a secret value, as a careless
/// provider error might.
#[derive(Debug, thiserror::Error)]
#[error("login with {0} failed")]
struct LeakyError(&'static str);

impl OperationError for LeakyError {
    fn code(&self) -> ErrorCodeName {
        "supervisor.spawn_failed".parse().expect("a catalog code")
    }

    fn retryable(&self) -> bool {
        false
    }

    fn details(&self) -> Vec<(DetailKey, String)> {
        vec![(
            "url".parse().expect("a detail key"),
            format!("postgres://app:{}@127.0.0.1/db", self.0),
        )]
    }
}

/// Fails its one step with a [`LeakyError`].
struct Leaky;

impl OperationWork for Leaky {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("login".parse().expect("a step name")))
                .await?;
            Err(WorkError::Failed(Box::new(LeakyError(LEAKED))))
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failure_is_stored_through_the_redactor_so_a_learned_value_never_lands() {
    let harness = Harness::new().await;
    harness
        .redactor
        .learn(&SecretValue::new(LEAKED.to_owned()).expect("a valid fixture value"));

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(Leaky),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    let error = failed
        .view
        .journal
        .failure_error()
        .expect("the failure error is journaled");
    assert_eq!(error.code().as_str(), "supervisor.spawn_failed");
    assert_eq!(error.message(), "login with **** failed");
    assert_eq!(error.details()[0].1, "postgres://app:****@127.0.0.1/db");
    assert!(!format!("{failed:?}").contains(LEAKED));
}

/// Arms a failing commit on the fake registry, then says the item is gone,
/// so the commit that settles it as compensated fails.
struct FullDiskCompensator {
    registry: Arc<FakeRegistry>,
}

impl Compensator for FullDiskCompensator {
    fn compensate(
        &self,
        _scope: CompensationScope,
        _record: &CreatedRecord,
    ) -> BoxFuture<'static, Result<CompensationResult, CompensationError>> {
        self.registry.fail_next_commit(RegistryError::NoSpace);
        Box::pin(async { Ok(CompensationResult::Compensated) })
    }
}

/// Records one custom item in a reversible step, then fails.
struct CustomThenFail;

impl OperationWork for CustomThenFail {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("mark".parse().expect("a step name")))
                .await?;
            let item = ctx
                .record_custom_intent(
                    "full_disk".parse().expect("a compensator name"),
                    "marker".parse().expect("an item key"),
                    None,
                )
                .await?;
            ctx.confirm_created(item).await?;
            Err(WorkError::Failed(TestError::boxed(
                "internal.unexpected",
                false,
            )))
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_item_settled_while_its_commit_failed_still_lands_in_the_registry() {
    let harness = Harness::new().await;
    harness.engine.register_compensator(
        "full_disk".parse().unwrap(),
        Arc::new(FullDiskCompensator {
            registry: Arc::clone(&harness.registry),
        }),
    );

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(CustomThenFail),
        ))
        .await;
    wait_state(&harness.engine, id, OperationState::Failed).await;
    let stored = harness
        .registry
        .operation_journal(id)
        .unwrap()
        .value
        .unwrap();

    assert_eq!(stored.created.len(), 1);
    assert_eq!(stored.created[0].state(), CreatedState::Compensated);
}

/// Passes its point of no return, then fails a repeatable step after it
/// armed a failing commit, so the commit that ends the step fails.
struct FailsOnAFullDisk {
    registry: Arc<FakeRegistry>,
}

impl OperationWork for FailsOnAFullDisk {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("fetch".parse().expect("a step name")).irreversible())
                .await?;
            ctx.finish_step(None).await?;
            ctx.begin_step(StepSpec::new("verify".parse().expect("a step name")).repeatable())
                .await?;
            self.registry.fail_next_commit(RegistryError::NoSpace);
            Err(WorkError::Failed(TestError::boxed(
                "runtime.checksum_mismatch",
                true,
            )))
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_step_ended_while_its_commit_failed_lands_and_resumes_after_a_restart() {
    let harness = Harness::new().await;
    let rebuilt = || {
        Arc::new(ScriptedFactory::always(
            ScriptedWork::new()
                .step("fetch")
                .irreversible()
                .step("verify")
                .repeatable(),
        ))
    };
    harness
        .engine
        .register_factory(OperationKind::InstallRuntime, rebuilt());

    let id = harness
        .begin(manager_request(
            OperationKind::InstallRuntime,
            Box::new(FailsOnAFullDisk {
                registry: Arc::clone(&harness.registry),
            }),
        ))
        .await;
    let stuck = wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;
    let stored = harness
        .registry
        .operation_journal(id)
        .unwrap()
        .value
        .unwrap();
    let (engine, _) = harness.restarted(crate::common::fast());
    engine.register_factory(OperationKind::InstallRuntime, rebuilt());
    engine.recover().await.unwrap();
    engine.start();
    engine
        .resume(id, Some(stuck.view.operation.revision()))
        .await
        .unwrap();
    let done = wait_state(&engine, id, OperationState::Succeeded).await;

    assert_eq!(stored.steps[1].state(), StepState::Failed);
    let attempts: Vec<(&str, u32)> = done
        .steps
        .iter()
        .map(|step| (step.name().as_str(), step.attempts()))
        .collect();
    assert_eq!(attempts, [("fetch", 1), ("verify", 2)]);
}

/// Begins one step more than a journal holds.
struct TooManySteps;

impl OperationWork for TooManySteps {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            for n in 1..=257 {
                ctx.begin_step(StepSpec::new(format!("s{n}").parse().expect("a step name")))
                    .await?;
                ctx.finish_step(None).await?;
            }
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

/// Ends its step with a checkpoint over the limit.
struct HugeCheckpoint;

impl OperationWork for HugeCheckpoint {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new("stage".parse().expect("a step name")))
                .await?;
            let checkpoint =
                JournalPayload::new(format!(r#"{{"blob":"{}"}}"#, "x".repeat(5_000)), usize::MAX)
                    .expect("a json object");
            ctx.finish_step(Some(checkpoint)).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_journal_over_its_limits_fails_with_its_own_catalog_code() {
    // Spec 0007 AC-16 and *Kode error*: journal_full with kind and max,
    // payload_too_large with field and max, not internal.unexpected.
    let harness = Harness::new().await;

    let steps = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(TooManySteps),
        ))
        .await;
    let full = wait_state(&harness.engine, steps, OperationState::Failed).await;
    let checkpoint = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(HugeCheckpoint),
        ))
        .await;
    let large = wait_state(&harness.engine, checkpoint, OperationState::Failed).await;

    let error_of = |detail: &stackly_core::operation::OperationDetailView| {
        let error = detail.view.journal.failure_error().expect("a stored error");
        let details: Vec<(String, String)> = error
            .details()
            .iter()
            .map(|(key, value)| (key.as_str().to_owned(), value.clone()))
            .collect();
        (error.code().as_str().to_owned(), details)
    };
    assert_eq!(
        error_of(&full),
        (
            "operation.journal_full".to_owned(),
            vec![
                ("kind".to_owned(), "steps".to_owned()),
                ("max".to_owned(), "256".to_owned())
            ]
        )
    );
    assert_eq!(
        error_of(&large),
        (
            "operation.payload_too_large".to_owned(),
            vec![
                ("field".to_owned(), "checkpoint".to_owned()),
                ("max".to_owned(), "4096".to_owned())
            ]
        )
    );
}

fn key(text: &str) -> DetailKey {
    text.parse().expect("a detail key")
}

fn item(kind: &str, fields: &[(&str, &str)]) -> FailureItem {
    FailureItem::new(
        kind.parse().expect("a reason code"),
        fields
            .iter()
            .map(|(name, value)| (key(name), (*value).to_owned()))
            .collect(),
    )
    .expect("a valid item")
}

/// Fails a manager operation at its one step with `items`, `rule`, and
/// `details`, and returns the failed operation.
async fn failed_with(
    harness: &Harness,
    details: Vec<(DetailKey, String)>,
    items: Vec<FailureItem>,
    rule: Option<Rule>,
) -> FailureError {
    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("check")
                .fail("provider.invalid_settings", false)
                .failing_with(details, items, rule)
                .boxed(),
        ))
        .await;
    wait_state(&harness.engine, id, OperationState::Failed)
        .await
        .view
        .journal
        .failure_error()
        .cloned()
        .expect("the failure error is journaled")
}

fn pairs(error: &FailureError) -> Vec<(&str, &str)> {
    error
        .details()
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_step_keeps_its_items_and_rule_and_a_restarted_engine_reads_them_back() {
    let harness = Harness::new().await;
    let items = vec![
        item("issue", &[("path", "/port"), ("detail", "out of range")]),
        item("issue", &[]),
    ];

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new()
                .step("check")
                .fail("provider.invalid_settings", false)
                .failing_with(
                    vec![(key("provider"), "official/postgresql".to_owned())],
                    items.clone(),
                    Some(Rule::Br002),
                )
                .boxed(),
        ))
        .await;
    let failed = wait_state(&harness.engine, id, OperationState::Failed).await;

    let error = failed.view.journal.failure_error().expect("an error");
    assert_eq!(error.items(), items.as_slice());
    assert_eq!(error.rule(), Some(Rule::Br002));
    assert_eq!(pairs(error), [("provider", "official/postgresql")]);

    let (restarted, _) = harness.restarted(fast());
    restarted.recover().await.expect("the registry recovers");
    let again = restarted.get(id).await.unwrap().expect("the operation");
    assert_eq!(
        again.view.journal.failure_error(),
        failed.view.journal.failure_error(),
        "the same error after a restart"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_without_items_or_rule_stores_none_and_engine_errors_never_carry_any() {
    let harness = Harness::new().await;

    let plain = failed_with(&harness, Vec::new(), Vec::new(), None).await;
    assert!(plain.items().is_empty() && plain.rule().is_none());
    assert!(
        plain.details().is_empty(),
        "no omitted detail without a drop"
    );

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            ScriptedWork::new().step("stage").panics().boxed(),
        ))
        .await;
    let panicked = wait_state(&harness.engine, id, OperationState::Failed).await;
    let error = panicked.view.journal.failure_error().expect("an error");
    assert_eq!(error.code().as_str(), "internal.unexpected");
    assert!(error.items().is_empty() && error.rule().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_with_101_items_keeps_100_and_counts_one_omitted() {
    let harness = Harness::new().await;
    let items: Vec<FailureItem> = (0..101)
        .map(|n| item("issue", &[("path", &format!("/field{n}"))]))
        .collect();

    let error = failed_with(&harness, Vec::new(), items.clone(), None).await;

    assert_eq!(error.items(), &items[..100]);
    assert_eq!(pairs(&error), [("omitted", "1")]);
}

/// covers: AC-16. The limit is 100 items, so exactly 100 keep every item and
/// count no omitted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_with_exactly_100_items_keeps_all_of_them_and_counts_no_omitted() {
    let harness = Harness::new().await;
    let items: Vec<FailureItem> = (0..100)
        .map(|n| item("issue", &[("path", &format!("/field{n}"))]))
        .collect();

    let error = failed_with(&harness, Vec::new(), items.clone(), None).await;

    assert_eq!(error.items(), items.as_slice());
    assert!(
        error.details().is_empty(),
        "no omitted detail without a drop"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn omitted_from_the_engine_adds_to_the_omitted_the_work_reported() {
    let harness = Harness::new().await;
    let items: Vec<FailureItem> = (0..102).map(|_| item("issue", &[])).collect();

    let error = failed_with(
        &harness,
        vec![(key("omitted"), "5".to_owned())],
        items,
        None,
    )
    .await;

    assert_eq!(error.items().len(), 100);
    assert_eq!(pairs(&error), [("omitted", "7")]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn items_over_65536_stored_bytes_are_dropped_from_the_back_and_counted() {
    let harness = Harness::new().await;
    let big = "v".repeat(512);
    let fields: Vec<(String, String)> = (0..8).map(|n| (format!("f{n}"), big.clone())).collect();
    let refs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let items: Vec<FailureItem> = (0..100).map(|_| item("issue", &refs)).collect();

    let error = failed_with(&harness, Vec::new(), items, None).await;

    let kept = error.items().len();
    assert!((1..100).contains(&kept), "kept {kept}");
    let omitted = 100 - kept;
    assert_eq!(pairs(&error), [("omitted", omitted.to_string().as_str())]);
    let stored = |count: usize| -> usize {
        let rows: Vec<serde_json::Value> = error.items()[..1]
            .iter()
            .cycle()
            .take(count)
            .map(|item| {
                serde_json::json!({
                    "kind": item.kind().as_str(),
                    "fields": item.fields().iter()
                        .map(|(name, value)| serde_json::json!({"key": name.as_str(), "value": value}))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        serde_json::to_string(&rows).expect("rows encode").len()
    };
    assert!(stored(kept) <= 65_536, "{} bytes", stored(kept));
    assert!(stored(kept + 1) > 65_536, "one more item would not fit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn item_values_go_through_the_redactor_and_never_reach_the_registry() {
    let harness = Harness::new().await;
    harness
        .redactor
        .learn(&SecretValue::new(LEAKED.to_owned()).expect("a valid fixture value"));
    let items = vec![item(
        "issue",
        &[("detail", &format!("password {LEAKED} rejected"))],
    )];

    let error = failed_with(&harness, Vec::new(), items, None).await;

    assert_eq!(error.items()[0].fields()[0].1, "password **** rejected");
    assert!(!format!("{error:?}").contains(LEAKED));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn items_and_rule_of_a_step_that_fails_after_the_point_of_no_return_are_kept() {
    let harness = Harness::new().await;
    let items = vec![item("reference", &[("name", "db-password")])];
    let mut begin = request(
        OperationKind::InstallRuntime,
        OperationResource::Runtime(RuntimeId::generate()),
        ScriptedWork::new()
            .step("fetch")
            .irreversible()
            .step("verify")
            .fail("provider.invalid_settings", false)
            .failing_with(Vec::new(), items.clone(), Some(Rule::Br018))
            .boxed(),
    );
    begin.creates_resource = true;

    let id = harness.begin(begin).await;
    let stuck = wait_state(&harness.engine, id, OperationState::NeedsRecovery).await;

    let error = stuck.view.journal.failure_error().expect("an error");
    assert_eq!(error.items(), items.as_slice());
    assert_eq!(error.rule(), Some(Rule::Br018));
}
