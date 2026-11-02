//! Daemon restart plans (spec 0030): a plan round trips with its instance
//! rows, the registry holds one open plan at most, updates rewrite the
//! state and the recorded fields, and a prune keeps the open plan and the
//! newest final ones.

use std::time::Duration;

use jiff::Timestamp;
use stackly_domain::{
    InstanceId, NewRestartPlan, OperationId, OperationState, PlanDecision, PlanInstance,
    PlanOutcome, PlanReason, PlanState, RegistryError, ResourceRef, RestartPlan, RestartPlanId,
    RetentionPolicy, WriteOp,
};
use stackly_platform::Clock;
use uuid::Uuid;

use super::{DAY, Suite, at, batch, domain, expect, expect_eq, parse, shifted};
use crate::registry::OperationBuilder;

/// The boot every plan of the suite was written in.
const BOOT: &str = "6f9619ff-8b86-4011-b42d-00c04fc964ff";

/// Runs every restart plan scenario.
pub(super) fn restart_plans(suite: &Suite<'_>) {
    let operation = OperationBuilder::new().build();
    suite.commit(
        "store the operation of the restart plans",
        vec![WriteOp::InsertOperation(operation.clone())],
    );
    let plan = lifecycle(suite, operation.id());
    one_open_plan(suite, operation.id());
    refusals(suite, operation.id(), &plan);
    prune(suite, operation.id());
    prune_keeps_referenced_operations(suite);
}

/// A plan in `stopping` with one `keep` and one `stop_restore` instance.
fn new_plan(operation: OperationId, created_at: Timestamp) -> RestartPlan {
    let entry = |name: &str, decision, reason| {
        PlanInstance::new(
            InstanceId::generate(),
            parse("make an instance name", name),
            decision,
            reason,
        )
    };
    RestartPlan::new(NewRestartPlan {
        id: RestartPlanId::generate(),
        operation,
        digest: parse("make a plan digest", &"9f".repeat(32)),
        boot_id: parse("make a boot ID", BOOT),
        server_instance_id_before: Uuid::now_v7(),
        created_at,
        instances: vec![
            entry("orders-db", PlanDecision::Keep, PlanReason::HandoverProven),
            entry(
                "cache",
                PlanDecision::StopRestore,
                PlanReason::HandoverUnproven,
            ),
        ],
    })
}

fn open(suite: &Suite<'_>, check: &str) -> Option<RestartPlan> {
    suite.value(check, suite.registry.open_restart_plan())
}

fn latest(suite: &Suite<'_>, check: &str) -> Option<RestartPlan> {
    suite.value(check, suite.registry.latest_restart_plan())
}

/// Writes a plan, moves it to `finished`, and reads it back at each step.
fn lifecycle(suite: &Suite<'_>, operation: OperationId) -> RestartPlan {
    const CHECK: &str = "a_restart_plan_round_trips_through_every_state";
    let mut plan = new_plan(operation, suite.loaded_at);
    suite.commit(CHECK, vec![WriteOp::InsertRestartPlan(plan.clone())]);
    expect_eq(CHECK, open(suite, CHECK), Some(plan.clone()));
    expect_eq(CHECK, latest(suite, CHECK), Some(plan.clone()));

    let [keep, stop] = [0, 1].map(|index| plan.instances()[index].clone());
    let (keep, stop) = if keep.decision == PlanDecision::Keep {
        (keep, stop)
    } else {
        (stop, keep)
    };
    let step = |seconds| shifted(suite.loaded_at, seconds);
    domain(CHECK, plan.record_stopped(stop.instance_id, step(1)));
    domain(CHECK, plan.hand_over(step(2)));
    suite.commit(CHECK, vec![WriteOp::UpdateRestartPlan(plan.clone())]);
    expect_eq(
        "a handed over plan is still open",
        open(suite, CHECK),
        Some(plan.clone()),
    );

    let boot = parse("make a boot ID", BOOT);
    domain(CHECK, plan.take_over(boot, Uuid::now_v7(), step(3)));
    domain(CHECK, plan.record_reconciled(step(4)));
    domain(
        CHECK,
        plan.record_outcome(
            keep.instance_id,
            PlanOutcome::Adopted,
            Some(step(4)),
            None,
            step(4),
        ),
    );
    domain(
        CHECK,
        plan.record_outcome(
            stop.instance_id,
            PlanOutcome::Failed,
            None,
            Some(parse("make a failure code", "data_missing")),
            step(5),
        ),
    );
    expect_eq(CHECK, plan.state(), PlanState::Finished);
    suite.commit(CHECK, vec![WriteOp::UpdateRestartPlan(plan.clone())]);
    expect_eq("a finished plan is not open", open(suite, CHECK), None);
    expect_eq(CHECK, latest(suite, CHECK), Some(plan.clone()));
    plan
}

/// A second open plan is refused while one is open, and allowed once the
/// first one is final.
fn one_open_plan(suite: &Suite<'_>, operation: OperationId) {
    const CHECK: &str = "the_registry_holds_at_most_one_open_restart_plan";
    let mut first = new_plan(operation, shifted(suite.loaded_at, 10));
    suite.commit(CHECK, vec![WriteOp::InsertRestartPlan(first.clone())]);
    let second = new_plan(operation, shifted(suite.loaded_at, 11));
    match suite
        .registry
        .commit(batch(vec![WriteOp::InsertRestartPlan(second.clone())]))
    {
        Err(err) => expect(
            CHECK,
            err.op_index == Some(0) && matches!(err.error, RegistryError::Integrity { .. }),
            || format!("expected Integrity at op 0, got {err:?}"),
        ),
        Ok(_) => expect(CHECK, false, || "a second open plan committed".to_owned()),
    }
    expect_eq(CHECK, open(suite, CHECK), Some(first.clone()));
    domain(CHECK, first.abort(shifted(suite.loaded_at, 12)));
    suite.commit(CHECK, vec![WriteOp::UpdateRestartPlan(first)]);
    suite.commit(CHECK, vec![WriteOp::InsertRestartPlan(second.clone())]);
    expect_eq(CHECK, open(suite, CHECK), Some(second.clone()));
    let mut second = second;
    domain(CHECK, second.abort(shifted(suite.loaded_at, 13)));
    suite.commit(CHECK, vec![WriteOp::UpdateRestartPlan(second)]);
}

/// The existence and reference checks of the two plan ops.
fn refusals(suite: &Suite<'_>, operation: OperationId, stored: &RestartPlan) {
    const CHECK: &str = "restart_plan_ops_check_existence_then_their_operation";
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertRestartPlan(stored.clone())]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::RestartPlan(stored.id()),
            },
        ),
    );
    let missing = OperationId::generate();
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertRestartPlan(new_plan(
            missing,
            suite.loaded_at,
        ))]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::Operation(missing),
            },
        ),
    );
    let unknown = new_plan(operation, suite.loaded_at);
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::UpdateRestartPlan(unknown.clone())]),
        at(
            0,
            RegistryError::NotFound {
                resource: ResourceRef::RestartPlan(unknown.id()),
            },
        ),
    );
}

/// A prune keeps the open plan and the newest final plans.
fn prune(suite: &Suite<'_>, operation: OperationId) {
    const CHECK: &str = "prune_keeps_the_open_plan_and_the_newest_final_plans";
    let open_plan = new_plan(operation, shifted(suite.loaded_at, 100));
    suite.commit(CHECK, vec![WriteOp::InsertRestartPlan(open_plan.clone())]);
    let keep_every_other = RetentionPolicy {
        operation_max_age: Duration::MAX,
        audit_max_age: Duration::MAX,
        released_lease_max_age: Duration::MAX,
        operation_max_rows: u32::MAX,
        audit_max_rows: u32::MAX,
        observations_per_instance: u32::MAX,
        restart_plans: 1,
    };
    let report = suite
        .registry
        .prune(&keep_every_other, suite.clock.now())
        .unwrap_or_else(|err| super::fail(CHECK, &err.to_string()));
    // The suite wrote three final plans; the newest one stays.
    expect_eq(CHECK, report.restart_plans, 2);
    expect_eq(CHECK, report.total(), 2);
    expect_eq(CHECK, open(suite, CHECK), Some(open_plan.clone()));
    let mut closed = open_plan;
    domain(CHECK, closed.abort(shifted(suite.loaded_at, 101)));
    suite.commit(CHECK, vec![WriteOp::UpdateRestartPlan(closed)]);
}

/// The retention of a plan is its own (AC-19): an operation older than the
/// age limit stays while a plan, open or final, still refers to it, and
/// goes once the plan is pruned.
fn prune_keeps_referenced_operations(suite: &Suite<'_>) {
    const CHECK: &str = "prune_keeps_the_operations_a_restart_plan_refers_to";
    let old = shifted(suite.clock.now(), -40 * DAY);
    let finished = || {
        OperationBuilder::new()
            .state(OperationState::Succeeded)
            .created_at(old)
            .updated_at(old)
            .build()
    };
    let (of_open, of_final, unreferenced) = (finished(), finished(), finished());
    suite.commit(
        CHECK,
        vec![
            WriteOp::InsertOperation(of_open.clone()),
            WriteOp::InsertOperation(of_final.clone()),
            WriteOp::InsertOperation(unreferenced.clone()),
        ],
    );
    let mut final_plan = new_plan(of_final.id(), shifted(suite.loaded_at, 200));
    domain(CHECK, final_plan.abort(shifted(suite.loaded_at, 201)));
    let mut open_plan = new_plan(of_open.id(), shifted(suite.loaded_at, 202));
    suite.commit(
        CHECK,
        vec![
            WriteOp::InsertRestartPlan(final_plan),
            WriteOp::InsertRestartPlan(open_plan.clone()),
        ],
    );
    let policy = |restart_plans| RetentionPolicy {
        operation_max_age: Duration::from_secs(30 * DAY as u64),
        audit_max_age: Duration::MAX,
        released_lease_max_age: Duration::MAX,
        operation_max_rows: u32::MAX,
        audit_max_rows: u32::MAX,
        observations_per_instance: u32::MAX,
        restart_plans,
    };
    let exists = |operation: &stackly_domain::Operation| {
        suite
            .value(CHECK, suite.registry.operation(operation.id()))
            .is_some()
    };

    suite
        .registry
        .prune(&policy(20), suite.clock.now())
        .unwrap_or_else(|err| super::fail(CHECK, &err.to_string()));

    expect(CHECK, exists(&of_open), || {
        "the operation of the open plan stays".to_owned()
    });
    expect(CHECK, exists(&of_final), || {
        "the operation of a final plan stays".to_owned()
    });
    expect(CHECK, !exists(&unreferenced), || {
        "an unreferenced old operation goes".to_owned()
    });
    expect_eq(CHECK, open(suite, CHECK), Some(open_plan.clone()));
    expect_eq(
        CHECK,
        latest(suite, CHECK).map(|plan| plan.id()),
        Some(open_plan.id()),
    );

    // Once the plans themselves are pruned their operations may go.
    domain(CHECK, open_plan.abort(shifted(suite.loaded_at, 203)));
    suite.commit(CHECK, vec![WriteOp::UpdateRestartPlan(open_plan)]);
    suite
        .registry
        .prune(&policy(0), suite.clock.now())
        .unwrap_or_else(|err| super::fail(CHECK, &err.to_string()));
    expect(CHECK, !exists(&of_open), || {
        "the operation of a pruned plan goes".to_owned()
    });
    expect(CHECK, !exists(&of_final), || {
        "the operation of a pruned plan goes".to_owned()
    });
}
