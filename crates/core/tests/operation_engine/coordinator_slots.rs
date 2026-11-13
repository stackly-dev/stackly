//! The coordinator slot class at the default limits (spec 0046 AC-3,
//! AC-24): eight ordinary operations fill every running slot and an Up
//! still starts, while a ninth ordinary operation queues.

use stackly_core::operation::limits::{MAX_RUNNING_COORDINATORS, MAX_RUNNING_OPERATIONS};
use stackly_core::operation::testing::{HoldHandle, ScriptedWork};
use stackly_domain::{
    OperationKind, OperationResource, OperationState, Registry, WriteBatch, WriteOp,
};
use stackly_testkit::EnvironmentBuilder;

use crate::common::{Harness, instance, request, wait_state};

// covers: AC-3, AC-24 (spec 0046)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_up_starts_while_eight_ordinary_operations_hold_every_running_slot() {
    let harness = Harness::new().await;
    assert_eq!(
        (MAX_RUNNING_OPERATIONS, MAX_RUNNING_COORDINATORS),
        (8, 4),
        "the default limits of spec 0007 and spec 0046"
    );
    let holds: Vec<HoldHandle> = (0..8).map(|_| HoldHandle::new()).collect();
    let mut ordinary = Vec::new();
    for hold in &holds {
        ordinary.push(
            harness
                .begin(request(
                    OperationKind::StartInstance,
                    OperationResource::Instance(instance(&harness)),
                    ScriptedWork::new().step("spawn").hold(hold).boxed(),
                ))
                .await,
        );
    }
    for hold in &holds {
        hold.started().await;
    }
    let ninth = harness
        .begin(request(
            OperationKind::StartInstance,
            OperationResource::Instance(instance(&harness)),
            ScriptedWork::new().step("spawn").boxed(),
        ))
        .await;

    let builder = EnvironmentBuilder::new().no_members();
    let environment = builder.id();
    harness
        .registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            builder.build(),
        )]))
        .expect("an environment can be stored");
    let up_hold = HoldHandle::new();
    let up = harness
        .begin(request(
            OperationKind::EnvironmentUp,
            OperationResource::Environment(environment),
            ScriptedWork::new().step("run").hold(&up_hold).boxed(),
        ))
        .await;
    up_hold.started().await;

    let stats = harness.engine.stats();
    assert_eq!(
        (stats.running, stats.max_running, stats.coordinators),
        (8, 8, 1),
        "the Up runs in its own class"
    );
    let queued = harness
        .engine
        .get(ninth)
        .await
        .expect("the engine reads")
        .expect("the ninth exists");
    assert_eq!(queued.view.operation.state(), OperationState::Queued);

    up_hold.release();
    wait_state(&harness.engine, up, OperationState::Succeeded).await;
    // The Up's end freed no ordinary slot: the ninth still waits.
    let still = harness
        .engine
        .get(ninth)
        .await
        .expect("the engine reads")
        .expect("the ninth exists");
    assert_eq!(still.view.operation.state(), OperationState::Queued);
    for hold in &holds {
        hold.release();
    }
    for id in ordinary {
        wait_state(&harness.engine, id, OperationState::Succeeded).await;
    }
    wait_state(&harness.engine, ninth, OperationState::Succeeded).await;
}
