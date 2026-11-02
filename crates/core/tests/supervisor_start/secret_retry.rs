//! The wait for a locked Keychain (spec 0031, AC-15): a login start that
//! meets a locked store leaves the instance `stopped` with desired
//! `running` and a `needsSecret` blocker, the supervisor reads the secrets
//! again every interval with no operation, and it starts the instance with
//! the trigger `secret_retry` when they read.

use std::time::Duration;

use stackly_core::supervisor::RegistryMode;
use stackly_domain::{
    DesiredState, InstanceBlocker, InstanceId, InstanceLifecycle, NeedsSecretReason, Registry,
    SecretRef, WriteBatch, WriteOp,
};

use crate::common::{Rig, Spec};

const INTERVAL: Duration = Duration::from_millis(150);

async fn rig() -> Rig {
    Rig::with_config(|config| config.login_secret_retry_interval = INTERVAL).await
}

fn wanted() -> Spec {
    Spec {
        lifecycle: InstanceLifecycle::Stopped,
        desired: DesiredState::Running,
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

fn reference(id: InstanceId) -> SecretRef {
    format!("local:test/{}", id.short_key())
        .parse()
        .expect("a test secret reference")
}

/// The reasons of the `needsSecret` blocker of `id`, if it has one.
fn reasons(rig: &Rig, id: InstanceId) -> Option<Vec<NeedsSecretReason>> {
    match rig.sup.status(id).blocker {
        Some(InstanceBlocker::NeedsSecret { references, .. }) => {
            Some(references.iter().map(|item| item.reason).collect())
        }
        _ => None,
    }
}

async fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..400 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// The login start meets the locked store.
async fn blocked_login_start(rig: &Rig, id: InstanceId) {
    rig.fake.secrets().lock();
    rig.sup
        .reconcile_in_session(RegistryMode::Ready, true)
        .await
        .expect("reconcile");
    rig.sup.queue_login_starts(true).await.expect("queue");
    rig.sup.start_reconciled();
    wait_for("the needsSecret blocker", || reasons(rig, id).is_some()).await;
}

/// The journal inputs of the `start_instance` operations, oldest last.
fn start_triggers(rig: &Rig) -> Vec<String> {
    let page = rig
        .registry
        .operations(stackly_domain::OperationQuery {
            active_only: false,
            resource: None,
            limit: 100,
            before: None,
        })
        .expect("operations read")
        .value;
    page.items
        .iter()
        .filter(|operation| operation.kind() == stackly_domain::OperationKind::StartInstance)
        .filter_map(|operation| {
            rig.registry
                .operation_journal(operation.id())
                .expect("the journal reads")
                .value
        })
        .map(|view| view.journal.input().as_str().to_owned())
        .collect()
}

fn operations(rig: &Rig) -> usize {
    rig.registry
        .operations(stackly_domain::OperationQuery {
            active_only: false,
            resource: None,
            limit: 100,
            before: None,
        })
        .expect("operations read")
        .value
        .items
        .len()
}

#[tokio::test]
async fn a_locked_keychain_leaves_the_instance_stopped_and_the_retry_starts_it_when_it_opens() {
    let rig = rig().await;
    let id = rig.instance(&wanted());
    blocked_login_start(&rig, id).await;

    assert_eq!(
        reasons(&rig, id),
        Some(vec![NeedsSecretReason::StoreLocked])
    );
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Running);

    // Several intervals pass while the store stays locked: the checks are not
    // operations, and nothing starts.
    let before = operations(&rig);
    tokio::time::sleep(INTERVAL * 5).await;
    assert_eq!(operations(&rig), before, "a check makes no operation");
    assert!(rig.processes().spawns().is_empty());
    assert_eq!(
        reasons(&rig, id),
        Some(vec![NeedsSecretReason::StoreLocked])
    );

    rig.fake.secrets().unlock();
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    wait_for("the instance to run", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
    })
    .await;
    assert_eq!(reasons(&rig, id), None, "the blocker went with the start");
    let triggers = start_triggers(&rig);
    assert_eq!(
        triggers
            .iter()
            .filter(|input| input.contains("secret_retry"))
            .count(),
        1,
        "exactly one start carries the trigger secret_retry: {triggers:?}"
    );
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Running);
}

#[tokio::test]
async fn a_stop_by_the_person_ends_the_wait_and_clears_the_blocker() {
    let rig = rig().await;
    let id = rig.instance(&wanted());
    blocked_login_start(&rig, id).await;

    let stored = rig
        .registry
        .instance(id)
        .expect("read")
        .value
        .expect("exists");
    let expected = stored.revision();
    let mut changed = stored.clone();
    changed.set_desired(DesiredState::Stopped);
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::UpdateInstance {
            instance: changed,
            expected,
        }]))
        .expect("the stop is written");

    wait_for("the blocker to go", || reasons(&rig, id).is_none()).await;
    rig.fake.secrets().unlock();
    tokio::time::sleep(INTERVAL * 4).await;
    assert!(rig.processes().spawns().is_empty(), "nothing starts");
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
}

#[tokio::test]
async fn a_reason_that_needs_the_person_ends_the_wait_and_keeps_the_desired_state() {
    let rig = rig().await;
    let id = rig.instance(&wanted());
    blocked_login_start(&rig, id).await;

    // The store opens, but the item no longer trusts this process.
    rig.fake.secrets().deny(&reference(id));
    rig.fake.secrets().unlock();
    wait_for("the reason to change", || {
        reasons(&rig, id) == Some(vec![NeedsSecretReason::AccessDenied])
    })
    .await;
    let reads = rig.fake.secrets().calls().len();
    tokio::time::sleep(INTERVAL * 5).await;

    assert_eq!(
        rig.fake.secrets().calls().len(),
        reads,
        "the supervisor reads no more once the person has to act"
    );
    assert!(rig.processes().spawns().is_empty());
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Running);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
}
