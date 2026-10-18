//! Retention: prune deletes finished history past its age and row limits,
//! keeps unfinished work, and never raises the generation (AC-14,
//! NFR-011).

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{
    Lease, LeaseFilter, Operation, OperationState, PruneReport, RetentionPolicy, WriteOp,
};
use stackly_platform::Clock;

use super::{DAY, Suite, expect_eq, fail, shifted};
use crate::registry::{LeaseBuilder, OperationBuilder};

/// How far the suite moves the clock before it prunes, so every row it
/// writes is far from what earlier scenarios wrote.
const PRUNE_ADVANCE_DAYS: i64 = 400;

pub(super) fn prune(suite: &Suite<'_>) {
    const CHECK: &str = "prune_keeps_unfinished_operations_and_bounds_history";
    suite
        .clock
        .advance(SignedDuration::from_secs(PRUNE_ADVANCE_DAYS * DAY));
    let now = suite.clock.now();
    let days_ago = |days: i64| shifted(now, -days * DAY);

    let everything = RetentionPolicy {
        operation_max_age: Duration::ZERO,
        audit_max_age: Duration::ZERO,
        released_lease_max_age: Duration::ZERO,
        operation_max_rows: 0,
        audit_max_rows: 0,
        observations_per_instance: 0,
    };
    run(
        suite,
        "prune all history earlier scenarios left",
        &everything,
        now,
    );
    expect_eq(
        "a second prune of everything finds nothing",
        run(suite, "prune all history again", &everything, now).total(),
        0,
    );

    let stack = suite.stack("store a runtime for prune");
    let busy = Suite::instance(&stack).build();
    let quiet = Suite::instance(&stack).build();
    let operation = |at: Timestamp, state| {
        OperationBuilder::new()
            .state(state)
            .created_at(at)
            .updated_at(at)
            .build()
    };
    let old_finished = [
        operation(days_ago(40), OperationState::Succeeded),
        operation(days_ago(41), OperationState::Failed),
    ];
    let recent_finished: Vec<Operation> = (1..=5)
        .map(|days| operation(days_ago(days), OperationState::Cancelled))
        .collect();
    let unfinished = [
        operation(days_ago(90), OperationState::NeedsRecovery),
        operation(days_ago(90), OperationState::Running),
        operation(days_ago(90), OperationState::Queued),
    ];
    let old_released = LeaseBuilder::new(busy.id())
        .acquired_at(days_ago(50))
        .released(days_ago(40))
        .build();
    let kept_leases = vec![
        LeaseBuilder::new(busy.id())
            .acquired_at(days_ago(90))
            .build(),
        LeaseBuilder::new(busy.id())
            .startup("session-1")
            .acquired_at(days_ago(80))
            .suppressed(days_ago(70))
            .build(),
        LeaseBuilder::new(busy.id())
            .acquired_at(days_ago(3))
            .released(days_ago(1))
            .build(),
    ];
    let mut ops = vec![
        WriteOp::InsertInstance(busy.clone()),
        WriteOp::InsertInstance(quiet.clone()),
        WriteOp::InsertLease(old_released.clone()),
    ];
    ops.extend(kept_leases.iter().cloned().map(WriteOp::InsertLease));
    ops.extend(
        old_finished
            .iter()
            .chain(&recent_finished)
            .chain(&unfinished)
            .cloned()
            .map(WriteOp::InsertOperation),
    );
    suite.commit("store history to prune", ops);
    suite.seed.seed_audit_events(3, days_ago(40));
    suite.seed.seed_audit_events(6, days_ago(1));
    let hour_ago = shifted(now, -3600);
    suite.seed.seed_observations(busy.id(), 5, hour_ago);
    suite.seed.seed_observations(quiet.id(), 1, hour_ago);

    let generation = suite.generation();
    let policy = RetentionPolicy {
        operation_max_age: days(30),
        audit_max_age: days(30),
        released_lease_max_age: days(30),
        operation_max_rows: 3,
        audit_max_rows: 4,
        observations_per_instance: 2,
    };
    let report = run(suite, CHECK, &policy, now);
    expect_eq(
        &format!("{CHECK}: finished operations past the age or row limit"),
        report.operations,
        4,
    );
    expect_eq(
        &format!("{CHECK}: audit events past the age or row limit"),
        report.audit_events,
        5,
    );
    expect_eq(
        &format!("{CHECK}: released leases past the age limit"),
        report.released_leases,
        1,
    );
    expect_eq(
        &format!("{CHECK}: process observations beyond the newest per instance"),
        report.process_observations,
        3,
    );
    expect_eq(
        &format!("{CHECK}: prune never raises the generation"),
        suite.generation(),
        generation,
    );
    expect_eq(
        &format!("{CHECK}: a second prune finds nothing"),
        run(suite, "prune again", &policy, now).total(),
        0,
    );
    expect_eq(
        &format!("{CHECK}: the default policy keeps what is left"),
        run(
            suite,
            "prune with the default policy",
            &RetentionPolicy::DEFAULT,
            now,
        )
        .total(),
        0,
    );

    for operation in &unfinished {
        expect_present(
            suite,
            &format!("{CHECK}: an unfinished operation stays, however old"),
            operation,
            true,
        );
    }
    for operation in &old_finished {
        expect_present(
            suite,
            &format!("{CHECK}: a finished operation past the age limit goes"),
            operation,
            false,
        );
    }
    for (index, operation) in recent_finished.iter().enumerate() {
        expect_present(
            suite,
            &format!("{CHECK}: only the newest finished operations within the row limit stay"),
            operation,
            index < 3,
        );
    }
    let leases: Vec<Lease> = suite.value(
        "list the leases after prune",
        suite.registry.leases(busy.id(), LeaseFilter::All),
    );
    expect_eq(
        &format!("{CHECK}: active, suppressed, and recently released leases stay"),
        leases,
        kept_leases,
    );
}

/// `days` whole days.
fn days(days: u64) -> Duration {
    Duration::from_secs(days * 24 * 60 * 60)
}

fn run(suite: &Suite<'_>, check: &str, policy: &RetentionPolicy, now: Timestamp) -> PruneReport {
    suite
        .registry
        .prune(policy, now)
        .unwrap_or_else(|err| fail(check, &err.to_string()))
}

fn expect_present(suite: &Suite<'_>, check: &str, operation: &Operation, present: bool) {
    let stored = suite.value(
        "read an operation after prune",
        suite.registry.operation(operation.id()),
    );
    expect_eq(check, stored.is_some(), present);
}
