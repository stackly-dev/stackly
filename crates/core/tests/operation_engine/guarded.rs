//! `OperationContext::commit_guarded` hands the registry's answer to the
//! work unchanged (spec 0020, "Commit terjaga di langkah `register`").

use std::sync::{Arc, Mutex, PoisonError};

use stackly_core::operation::{
    BoxFuture, EngineError, OperationContext, OperationOutcome, OperationWork, StepSpec, WorkError,
    WorkOutcome, WorkResult,
};
use stackly_domain::{
    CommitGuard, GuardDecision, Guarded, OperationKind, OperationState, Registry, RegistryError,
    WriteOp,
};
use stackly_testkit::{EnvironmentBuilder, FakeRegistry};

use crate::common::{Harness, manager_request, wait_state};

/// Runs a refusing, a writing, and a failing guarded commit and records
/// what each returned.
struct Guarding {
    registry: Arc<FakeRegistry>,
    env: EnvironmentBuilder,
    seen: Arc<Mutex<Vec<String>>>,
}

impl OperationWork for Guarding {
    fn run(self: Box<Self>, mut ctx: OperationContext) -> BoxFuture<'static, WorkResult> {
        Box::pin(async move {
            ctx.begin_step(StepSpec::new(
                "guard".parse().expect("guard step name is valid"),
            ))
            .await?;
            let note = |text: String| {
                self.seen
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(text);
            };

            let refuse: CommitGuard =
                Box::new(|_| Ok(GuardDecision::Refuse(Box::new(String::from("not now")))));
            match ctx.commit_guarded(refuse).await? {
                Guarded::Refused(reason) => note(format!(
                    "refused:{}",
                    reason.downcast_ref::<String>().cloned().unwrap_or_default()
                )),
                Guarded::Committed(_) => note("committed".to_owned()),
            }

            let op = WriteOp::InsertEnvironment(self.env.clone().build());
            let write: CommitGuard = Box::new(move |_| Ok(GuardDecision::Write(vec![op])));
            match ctx.commit_guarded(write).await? {
                Guarded::Committed(committed) => {
                    note(format!("committed:{}", committed.revisions.len()));
                }
                Guarded::Refused(_) => note("refused".to_owned()),
            }

            self.registry.fail_next_commit(RegistryError::Busy);
            let failing: CommitGuard = Box::new(|_| Ok(GuardDecision::Write(Vec::new())));
            match ctx.commit_guarded(failing).await {
                Err(WorkError::Engine(EngineError::Registry(error))) => {
                    note(format!("registry:{error}"));
                }
                other => note(format!("unexpected:{other:?}")),
            }

            ctx.finish_step(None).await?;
            Ok(WorkOutcome {
                result: OperationOutcome::default(),
                finalize: None,
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_guarded_commit_passes_committed_and_refused_through_and_maps_a_registry_error() {
    let harness = Harness::new().await;
    let env = EnvironmentBuilder::new().no_members();
    let seen = Arc::new(Mutex::new(Vec::new()));

    let id = harness
        .begin(manager_request(
            OperationKind::UpdateManager,
            Box::new(Guarding {
                registry: Arc::clone(&harness.registry),
                env: env.clone(),
                seen: Arc::clone(&seen),
            }),
        ))
        .await;
    wait_state(&harness.engine, id, OperationState::Succeeded).await;

    let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(
        seen,
        vec![
            "refused:not now".to_owned(),
            "committed:1".to_owned(),
            format!("registry:{}", RegistryError::Busy),
        ]
    );
    assert!(
        harness
            .registry
            .environment(env.id())
            .expect("the environment reads")
            .value
            .is_some(),
        "the guarded write landed"
    );
}
