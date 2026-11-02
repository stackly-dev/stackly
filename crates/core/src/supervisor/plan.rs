//! The guarded write of a daemon restart plan (spec 0030, `NoOpenRestartPlan`):
//! inside one registry transaction the guard checks that no plan is open
//! and that the plan's digest still matches the registry, then writes it.

use stackly_domain::{
    CommitGuard, GuardDecision, GuardView, RegistryError, RestartPlan, RestartPlanId, Sha256Digest,
    WriteOp,
};

/// Why [`write_plan_guard`] refused, handed back as
/// [`Guarded::Refused`](stackly_domain::Guarded::Refused).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanRefusal {
    /// Another plan is open.
    OpenPlan(RestartPlanId),
    /// The plan computed again from the registry has another digest.
    DigestChanged {
        /// The digest of the plan being written.
        expected: Sha256Digest,
        /// The digest computed inside the transaction.
        actual: Sha256Digest,
    },
}

/// Returns the guard that writes `plan`, with `extra` ops after it, unless
/// a plan is open or `recompute` gives the registry's plan another digest.
///
/// `recompute` reads only through the [`GuardView`], like every guard.
pub fn write_plan_guard<F>(plan: RestartPlan, extra: Vec<WriteOp>, recompute: F) -> CommitGuard
where
    F: FnOnce(&dyn GuardView) -> Result<Sha256Digest, RegistryError> + Send + 'static,
{
    Box::new(move |view: &dyn GuardView| {
        if let Some(open) = view.open_restart_plan()? {
            return Ok(GuardDecision::Refuse(Box::new(PlanRefusal::OpenPlan(
                open.id(),
            ))));
        }
        let expected = plan.stored().digest;
        let actual = recompute(view)?;
        if actual != expected {
            return Ok(GuardDecision::Refuse(Box::new(
                PlanRefusal::DigestChanged { expected, actual },
            )));
        }
        let mut ops = vec![WriteOp::InsertRestartPlan(plan)];
        ops.extend(extra);
        Ok(GuardDecision::Write(ops))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jiff::Timestamp;
    use stackly_domain::{Guarded, NewRestartPlan, OperationId, Registry, WriteBatch};
    use stackly_platform::Clock;
    use stackly_testkit::{FakeClock, FakeRegistry, OperationBuilder};
    use uuid::Uuid;

    use super::*;

    fn digest(byte: &str) -> Sha256Digest {
        byte.repeat(32).parse().unwrap()
    }

    fn plan(operation: OperationId, created_at: Timestamp) -> RestartPlan {
        RestartPlan::new(NewRestartPlan {
            id: RestartPlanId::generate(),
            operation,
            digest: digest("ab"),
            boot_id: "6f9619ff-8b86-4011-b42d-00c04fc964ff".parse().unwrap(),
            server_instance_id_before: Uuid::now_v7(),
            created_at,
            instances: Vec::new(),
        })
    }

    fn registry() -> (FakeRegistry, OperationId, Timestamp) {
        let clock = FakeClock::new();
        let now = clock.now();
        let registry = FakeRegistry::new(Arc::new(clock), now);
        let operation = OperationBuilder::new().build();
        let id = operation.id();
        registry
            .commit(WriteBatch::new(vec![WriteOp::InsertOperation(operation)]))
            .unwrap();
        (registry, id, now)
    }

    fn refusal(guarded: Guarded) -> PlanRefusal {
        match guarded {
            Guarded::Refused(reason) => *reason.downcast::<PlanRefusal>().unwrap(),
            Guarded::Committed(_) => panic!("the guard wrote"),
        }
    }

    #[test]
    fn a_plan_is_written_when_none_is_open_and_its_digest_still_matches() {
        let (registry, operation, now) = registry();
        let first = plan(operation, now);
        let guard = write_plan_guard(first.clone(), Vec::new(), |_| Ok(digest("ab")));
        assert!(matches!(
            registry.commit_guarded(guard).unwrap(),
            Guarded::Committed(_)
        ));
        assert_eq!(
            registry.open_restart_plan().unwrap().value,
            Some(first.clone())
        );

        let second = plan(operation, now);
        let guard = write_plan_guard(second, Vec::new(), |_| Ok(digest("ab")));
        assert_eq!(
            refusal(registry.commit_guarded(guard).unwrap()),
            PlanRefusal::OpenPlan(first.id())
        );
    }

    #[test]
    fn a_digest_that_changed_inside_the_transaction_refuses_the_plan() {
        let (registry, operation, now) = registry();
        let guard = write_plan_guard(plan(operation, now), Vec::new(), |_| Ok(digest("cd")));
        assert_eq!(
            refusal(registry.commit_guarded(guard).unwrap()),
            PlanRefusal::DigestChanged {
                expected: digest("ab"),
                actual: digest("cd"),
            }
        );
        assert_eq!(registry.open_restart_plan().unwrap().value, None);
    }
}
