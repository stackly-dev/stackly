//! Guarded commits: a guard reads and the ops it returns write in one
//! transaction (spec 0020, "Commit terjaga di langkah `register`", AC-10).

use std::sync::Barrier;
use std::thread;

use stackly_domain::{
    BatchError, CommitGuard, GuardDecision, Guarded, Instance, InstanceFilter, InstanceId,
    InstanceName, RegistryError, WriteOp,
};

use super::{Suite, at, expect, expect_eq, fail, parse, suffix};

/// How many threads race, and how many of them may write.
const RACERS: usize = 16;
const ALLOWED: usize = 8;

pub(super) fn guarded(suite: &Suite<'_>) {
    sees_earlier_batches(suite);
    refusal_applies_nothing(suite);
    failing_op_applies_nothing(suite);
    guard_error_applies_nothing(suite);
    guard_panic_keeps_serving(suite);
    sixteen_guards_admit_eight(suite);
}

/// A guard that always writes `instance`.
fn writes(instance: &Instance) -> CommitGuard {
    let op = WriteOp::InsertInstance(instance.clone());
    Box::new(move |_| Ok(GuardDecision::Write(vec![op])))
}

/// A guarded commit that must succeed.
fn committed(check: &str, suite: &Suite<'_>, guard: CommitGuard) -> stackly_domain::Committed {
    match suite.registry.commit_guarded(guard) {
        Ok(Guarded::Committed(committed)) => committed,
        Ok(Guarded::Refused(_)) => fail(check, "the guard refused"),
        Err(err) => fail(check, &err.to_string()),
    }
}

/// The guard sees what committed before it, and its writes land at the
/// generation after.
fn sees_earlier_batches(suite: &Suite<'_>) {
    const CHECK: &str = "a_guard_sees_earlier_commits_and_its_writes_raise_the_generation";
    let stack = suite.stack("store a runtime for the guarded commit");
    let first = Suite::instance(&stack).build();
    suite.commit(
        "store the first instance",
        vec![WriteOp::InsertInstance(first.clone())],
    );
    let second = Suite::instance(&stack).build();
    let before = suite.generation();
    let seen = first.id();
    let runtime = stack.runtime.id();
    let guard: CommitGuard = {
        let op = WriteOp::InsertInstance(second.clone());
        Box::new(move |view| {
            let instances = view.instances(InstanceFilter::default())?;
            if !instances.iter().any(|instance| instance.id() == seen) {
                return Ok(GuardDecision::Refuse(Box::new("first instance missing")));
            }
            if view.runtime(runtime)?.is_none() {
                return Ok(GuardDecision::Refuse(Box::new("runtime missing")));
            }
            Ok(GuardDecision::Write(vec![op]))
        })
    };

    let result = committed(CHECK, suite, guard);

    expect_eq(CHECK, result.generation, before.next());
    expect_eq(
        &format!("{CHECK}: the write is stored"),
        suite.value("read the instance", suite.registry.instance(second.id())),
        Some(suite.expected_instance(&second)),
    );
}

/// A refusal applies nothing, keeps the generation, and returns its reason.
fn refusal_applies_nothing(suite: &Suite<'_>) {
    const CHECK: &str = "a_refusing_guard_applies_nothing_and_returns_its_reason";
    let stack = suite.stack("store a runtime for the refusal");
    let instance = Suite::instance(&stack).build();
    let before = suite.generation();
    let guard: CommitGuard = Box::new(|_| Ok(GuardDecision::Refuse(Box::new(String::from("no")))));

    let outcome = suite.registry.commit_guarded(guard);

    match outcome {
        Ok(Guarded::Refused(reason)) => {
            expect_eq(
                CHECK,
                reason.downcast_ref::<String>().map(String::as_str),
                Some("no"),
            );
        }
        other => fail(CHECK, &format!("expected a refusal, got {other:?}")),
    }
    expect_eq(&format!("{CHECK}: generation"), suite.generation(), before);
    expect_absent(CHECK, suite, instance.id());
}

/// A failing op applies none of the ops and returns its index.
fn failing_op_applies_nothing(suite: &Suite<'_>) {
    const CHECK: &str = "a_guarded_commit_whose_op_fails_applies_nothing";
    let stack = suite.stack("store a runtime for the failing op");
    let name: InstanceName = parse(
        "make a name",
        &format!("guard-op-{}", suffix(stack.runtime.id())),
    );
    let first = Suite::instance(&stack).name(name.as_str()).build();
    let twin = Suite::instance(&stack).name(name.as_str()).build();
    let before = suite.generation();
    let ops = vec![
        WriteOp::InsertInstance(first.clone()),
        WriteOp::InsertInstance(twin),
    ];
    let guard: CommitGuard = Box::new(move |_| Ok(GuardDecision::Write(ops)));

    let outcome = suite.registry.commit_guarded(guard);

    match outcome {
        Err(err) => expect_eq(
            CHECK,
            err,
            at(
                1,
                RegistryError::NameTaken {
                    name,
                    existing: first.id(),
                },
            ),
        ),
        other => fail(CHECK, &format!("expected NameTaken, got {other:?}")),
    }
    expect_eq(&format!("{CHECK}: generation"), suite.generation(), before);
    expect_absent(CHECK, suite, first.id());
}

/// An `Err` from the guard comes back as a batch error with no writes.
fn guard_error_applies_nothing(suite: &Suite<'_>) {
    const CHECK: &str = "a_guard_error_returns_as_a_batch_error";
    let before = suite.generation();
    let guard: CommitGuard = Box::new(|_| Err(RegistryError::Busy));

    let outcome = suite.registry.commit_guarded(guard);

    match outcome {
        Err(err) => expect_eq(CHECK, err, BatchError::batch(RegistryError::Busy)),
        other => fail(CHECK, &format!("expected Busy, got {other:?}")),
    }
    expect_eq(&format!("{CHECK}: generation"), suite.generation(), before);
}

/// A panicking guard becomes `Io`, and the registry serves on.
fn guard_panic_keeps_serving(suite: &Suite<'_>) {
    const CHECK: &str = "a_panicking_guard_becomes_io_and_the_registry_keeps_serving";
    let stack = suite.stack("store a runtime for the panic");
    let before = suite.generation();
    let guard: CommitGuard = Box::new(|_| panic!("simulated guard panic"));

    let outcome = suite.registry.commit_guarded(guard);

    match outcome {
        Err(err) => expect_eq(
            CHECK,
            err,
            BatchError::batch(RegistryError::Io {
                detail: "the commit guard panicked".to_owned(),
            }),
        ),
        other => fail(CHECK, &format!("expected Io, got {other:?}")),
    }
    expect_eq(&format!("{CHECK}: generation"), suite.generation(), before);
    let after = Suite::instance(&stack).build();
    committed(CHECK, suite, writes(&after));
    expect_eq(
        &format!("{CHECK}: a later guarded commit lands"),
        suite.value("read the instance", suite.registry.instance(after.id())),
        Some(suite.expected_instance(&after)),
    );
}

/// 16 guards that each admit their instance only while fewer than 8 with
/// the run's prefix exist: 8 commit, 8 are refused, none errors.
fn sixteen_guards_admit_eight(suite: &Suite<'_>) {
    const CHECK: &str = "concurrent_guards_admit_exactly_the_allowed_count";
    let stack = suite.stack("store a runtime for the guard race");
    let run = suffix(InstanceId::generate()).to_lowercase();
    let prefix = format!("guard-{run}-");
    let instances: Vec<Instance> = (0..RACERS)
        .map(|index| {
            Suite::instance(&stack)
                .name(&format!("{prefix}{index}"))
                .build()
        })
        .collect();
    let registry = suite.registry;
    let start = Barrier::new(RACERS);
    let results: Vec<Result<Guarded, BatchError>> = thread::scope(|scope| {
        let racers: Vec<_> = instances
            .iter()
            .map(|instance| {
                let start = &start;
                let prefix = prefix.clone();
                let op = WriteOp::InsertInstance(instance.clone());
                scope.spawn(move || {
                    let guard: CommitGuard = Box::new(move |view| {
                        let taken = view
                            .instances(InstanceFilter::default())?
                            .iter()
                            .filter(|instance| instance.name().as_str().starts_with(&prefix))
                            .count();
                        if taken < ALLOWED {
                            Ok(GuardDecision::Write(vec![op]))
                        } else {
                            Ok(GuardDecision::Refuse(Box::new(taken)))
                        }
                    });
                    start.wait();
                    registry.commit_guarded(guard)
                })
            })
            .collect();
        racers
            .into_iter()
            .map(|racer| {
                racer
                    .join()
                    .unwrap_or_else(|_| fail(CHECK, "a racing thread panicked"))
            })
            .collect()
    });

    let count = |wanted: fn(&Result<Guarded, BatchError>) -> bool| {
        results.iter().filter(|result| wanted(result)).count()
    };
    expect_eq(
        &format!("{CHECK}: committed"),
        count(|result| matches!(result, Ok(Guarded::Committed(_)))),
        ALLOWED,
    );
    expect_eq(
        &format!("{CHECK}: refused"),
        count(|result| matches!(result, Ok(Guarded::Refused(_)))),
        RACERS - ALLOWED,
    );
    expect_eq(
        &format!("{CHECK}: batch errors"),
        count(|result| result.is_err()),
        0,
    );
    let stored = suite
        .value(
            "list the instances",
            suite.registry.instances(InstanceFilter::default()),
        )
        .iter()
        .filter(|instance| instance.name().as_str().starts_with(&prefix))
        .count();
    expect_eq(&format!("{CHECK}: stored"), stored, ALLOWED);
}

/// The instance is not in the registry.
fn expect_absent(check: &str, suite: &Suite<'_>, id: InstanceId) {
    let stored = suite.value("read the instance", suite.registry.instance(id));
    expect(check, stored.is_none(), || {
        "the instance was stored although the commit applied nothing".to_owned()
    });
}
