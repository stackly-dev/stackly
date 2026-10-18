//! Races of 16 threads that commit at the same moment: exactly one wins
//! and the rest fail with a typed error (AC-9, NFR-005, REQ-048).

use std::sync::Barrier;
use std::thread;

use stackly_domain::{
    BatchError, Committed, Instance, InstanceName, RegistryError, ResourceRef, Runtime, WriteBatch,
    WriteOp,
};

use super::{Suite, at, batch, expect_eq, fail, parse, rev, suffix};
use crate::registry::RuntimeBuilder;

/// How many threads race.
const RACERS: usize = 16;

pub(super) fn races(suite: &Suite<'_>) {
    same_name(suite);
    same_artifact(suite);
    same_revision(suite);
}

/// 16 creates of instances with one name: one succeeds, the rest get
/// `NameTaken`.
fn same_name(suite: &Suite<'_>) {
    const CHECK: &str = "concurrent_creates_produce_one_instance";
    let stack = suite.stack("store a runtime for the name race");
    let name: InstanceName = parse(
        "make a name",
        &format!("race-{}", suffix(stack.runtime.id())),
    );
    let instances: Vec<Instance> = (0..RACERS)
        .map(|_| Suite::instance(&stack).name(name.as_str()).build())
        .collect();
    let results = race(
        suite,
        instances
            .iter()
            .map(|instance| batch(vec![WriteOp::InsertInstance(instance.clone())]))
            .collect(),
    );
    let winner = &instances[sole_winner(CHECK, &results)];
    expect_losers(
        CHECK,
        &results,
        &at(
            0,
            RegistryError::NameTaken {
                name: name.clone(),
                existing: winner.id(),
            },
        ),
    );
    expect_eq(
        &format!("{CHECK}: the winner holds the name"),
        suite
            .value("find the name", suite.registry.instance_by_name(&name))
            .map(|instance| instance.id()),
        Some(winner.id()),
    );
}

/// 16 creates of managed runtimes for one artifact: one succeeds, the rest
/// get `DuplicateRuntime`.
fn same_artifact(suite: &Suite<'_>) {
    const CHECK: &str = "concurrent_runtime_installs_produce_one_runtime";
    let artifact = RuntimeBuilder::new().artifact();
    suite.commit(
        "store an artifact for the runtime race",
        vec![WriteOp::InsertArtifact(artifact.clone())],
    );
    let runtimes: Vec<Runtime> = (0..RACERS)
        .map(|_| RuntimeBuilder::new().managed(artifact.sha256).build())
        .collect();
    let results = race(
        suite,
        runtimes
            .iter()
            .map(|runtime| batch(vec![WriteOp::InsertRuntime(runtime.clone())]))
            .collect(),
    );
    let winner = &runtimes[sole_winner(CHECK, &results)];
    expect_losers(
        CHECK,
        &results,
        &at(
            0,
            RegistryError::DuplicateRuntime {
                existing: winner.id(),
            },
        ),
    );
}

/// 16 updates of one instance from the same expected revision: one
/// succeeds, the rest get `RevisionConflict`.
fn same_revision(suite: &Suite<'_>) {
    const CHECK: &str = "concurrent_updates_from_one_revision_apply_once";
    let stack = suite.stack("store a runtime for the revision race");
    let target = Suite::instance(&stack).build();
    let id = target.id();
    suite.commit(
        "store an instance for the revision race",
        vec![WriteOp::InsertInstance(target.clone())],
    );
    let updates: Vec<Instance> = (0..RACERS)
        .map(|racer| {
            let mut renamed = target.clone();
            renamed.rename(parse(
                "make a name",
                &format!("rename-{racer}-{}", suffix(id)),
            ));
            renamed
        })
        .collect();
    let results = race(
        suite,
        updates
            .iter()
            .map(|instance| {
                batch(vec![WriteOp::UpdateInstance {
                    instance: instance.clone(),
                    expected: rev(1),
                }])
            })
            .collect(),
    );
    let winner = &updates[sole_winner(CHECK, &results)];
    expect_losers(
        CHECK,
        &results,
        &at(
            0,
            RegistryError::RevisionConflict {
                resource: ResourceRef::Instance(id),
                expected: rev(1),
                actual: rev(2),
            },
        ),
    );
    expect_eq(
        &format!("{CHECK}: the winner's update is stored"),
        suite.value("read the instance", suite.registry.instance(id)),
        Some(suite.expected_instance(winner)),
    );
}

/// Commits every batch from its own thread, all released at once.
fn race(suite: &Suite<'_>, batches: Vec<WriteBatch>) -> Vec<Result<Committed, BatchError>> {
    let registry = suite.registry;
    let start = Barrier::new(batches.len());
    thread::scope(|scope| {
        let racers: Vec<_> = batches
            .into_iter()
            .map(|batch| {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    registry.commit(batch)
                })
            })
            .collect();
        racers
            .into_iter()
            .map(|racer| {
                racer
                    .join()
                    .unwrap_or_else(|_| fail("race commits", "a racing thread panicked"))
            })
            .collect()
    })
}

/// Returns the index of the only successful commit.
fn sole_winner(check: &str, results: &[Result<Committed, BatchError>]) -> usize {
    let winners: Vec<usize> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.is_ok())
        .map(|(index, _)| index)
        .collect();
    match winners.as_slice() {
        [winner] => *winner,
        other => fail(
            &format!("{check}: exactly one commit succeeds"),
            &format!("{} succeeded", other.len()),
        ),
    }
}

/// Checks that every failed commit failed with `expected`.
fn expect_losers(check: &str, results: &[Result<Committed, BatchError>], expected: &BatchError) {
    for result in results {
        if let Err(err) = result {
            expect_eq(
                &format!("{check}: every other commit fails the same way"),
                err,
                expected,
            );
        }
    }
}
