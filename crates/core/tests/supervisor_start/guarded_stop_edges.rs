//! More of the stop with a lease guard (spec 0047, *Stop dengan guard
//! lease*), next to `guarded_stop`: the three commit places with each
//! `LeaseCheck` (a live process, a process that is gone before the stop
//! holds the lock, and the quiet branch), the respawn that is cancelled only
//! after `Committed`, and the actions that never read a lease (AC-14).

use std::time::Duration;

use stackly_core::operation::{OperationOutcome, PartialOutcome};
use stackly_core::supervisor::{StopAllRequest, StopRequest, codes};
use stackly_domain::{
    DesiredState, EnvironmentId, InstanceId, InstanceLifecycle, Lease, LeaseFilter, LeaseState,
    OperationId, OperationKind, OperationState, Registry, ResourceRef, WriteBatch, WriteOp,
};
use stackly_platform::{ChildRole, ExitStatus, ProviderSignal};
use stackly_testkit::{EnvironmentBuilder, FakeSpawn, LeaseBuilder};

use crate::common::{Rig, Spec, replace};

fn spec() -> Spec {
    Spec {
        lifecycle: InstanceLifecycle::Stopped,
        desired: DesiredState::Stopped,
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn running(rig: &Rig, id: InstanceId, nth: usize) -> FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(nth).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    spawn
}

/// Stores an environment named `name` that shares `id`, and its active
/// lease on it.
fn environment_lease(rig: &Rig, id: InstanceId, name: &str) -> (EnvironmentId, Lease) {
    let environment = EnvironmentBuilder::new().name(name).shared(id).build();
    let lease = LeaseBuilder::new(id).environment(environment.id()).build();
    rig.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertEnvironment(environment.clone()),
            WriteOp::InsertLease(lease.clone()),
        ]))
        .expect("the environment and its lease are stored");
    (environment.id(), lease)
}

fn lease_state(rig: &Rig, id: InstanceId, lease: &Lease) -> LeaseState {
    rig.registry
        .leases(id, LeaseFilter::All)
        .expect("leases read")
        .value
        .into_iter()
        .find(|stored| stored.id() == lease.id())
        .expect("the lease is stored")
        .state()
}

/// Releases every unreleased lease of `id` except `keep`, so only `keep`
/// holds it (the start of the rig leaves a manual lease).
fn release_all_but(rig: &Rig, id: InstanceId, keep: &Lease) {
    let now = rig.fake_now();
    let released: Vec<WriteOp> = rig
        .registry
        .leases(id, LeaseFilter::Unreleased)
        .expect("leases read")
        .value
        .into_iter()
        .filter(|lease| lease.id() != keep.id())
        .map(|mut lease| {
            lease.release(now).expect("an active lease releases");
            WriteOp::UpdateLease(lease)
        })
        .collect();
    if !released.is_empty() {
        rig.registry
            .commit(WriteBatch::new(released))
            .expect("the other leases are released");
    }
}

async fn stop_with(rig: &Rig, request: StopRequest) -> OperationId {
    let id = request.instance;
    let work = rig.sup.stop_work(request);
    rig.begin(OperationKind::StopInstance, id, work).await
}

fn outcome(detail: &stackly_core::operation::OperationDetailView) -> OperationOutcome {
    OperationOutcome::from_payload(detail.view.journal.result().expect("a result"))
        .expect("an outcome")
}

fn main_spawns(rig: &Rig) -> usize {
    rig.processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count()
}

/// Starts `id`, then crashes its process so it waits `recovering`.
async fn recovering(rig: &Rig, id: InstanceId) {
    let first = running(rig, id, 1).await;
    rig.kill_servers();
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the instance recovers", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
    })
    .await;
}

async fn slow_respawn_rig() -> Rig {
    Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_millis(1500);
        config.restart_max_delay = Duration::from_millis(1500);
    })
    .await
}

async fn stop_all_of(
    rig: &Rig,
    id: InstanceId,
    request: StopAllRequest,
) -> stackly_core::operation::OperationDetailView {
    use stackly_core::operation::{BeginOutcome, BeginRequest, BusyPolicy, Origin};
    use stackly_domain::{JournalPayload, LockKey, OperationResource};

    let begun = rig
        .engine
        .begin(BeginRequest {
            kind: OperationKind::StopAllInstances,
            resource: OperationResource::Manager,
            creates_resource: false,
            extra_locks: vec![LockKey::for_resource(OperationResource::Instance(id))],
            busy: BusyPolicy::Queue,
            expected_revision: None,
            origin: Origin::Background,
            input: JournalPayload::empty(),
            work: rig.sup.stop_all_work(request),
            precheck: None,
        })
        .await
        .expect("the engine accepts the operation");
    let BeginOutcome::Created(view) = begun else {
        panic!("expected a created operation, got {begun:?}");
    };
    rig.wait_state(view.operation.id(), OperationState::Succeeded)
        .await
}

// covers: AC-13 (spec 0047), `settle_gone_process` with `LeaseCheck::Manual`:
// a lease that lands after the precheck, on an instance whose process is
// already gone, disturbs nobody, so the guard commits the stop without the
// override and the environment's lease stays
#[tokio::test]
async fn a_persons_stop_whose_process_is_gone_commits_without_the_override_and_keeps_a_late_lease()
{
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Stopping,
        desired: DesiredState::Running,
        ..spec()
    });
    let environment = EnvironmentBuilder::new().name("late").shared(id).build();
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            environment.clone(),
        )]))
        .expect("the environment is stored");
    let lease = LeaseBuilder::new(id).environment(environment.id()).build();
    rig.registry
        .before_guarded_commit(WriteBatch::new(vec![WriteOp::InsertLease(lease.clone())]));

    let op = rig.stop(id).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let row = rig.instance_row(id);
    assert_eq!(
        row.lifecycle(),
        InstanceLifecycle::Stopped,
        "never left stopping"
    );
    assert_eq!(row.desired(), DesiredState::Stopped);
    assert_eq!(
        lease_state(&rig, id, &lease),
        LeaseState::Active,
        "a person's stop never releases an environment's lease"
    );
}

// covers: AC-13 (spec 0047): `stopping` counts as live at the precheck, so
// without the override the stop is refused there and nothing is written
#[tokio::test]
async fn a_persons_stop_of_a_stopping_instance_an_environment_holds_needs_the_override() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Stopping,
        desired: DesiredState::Running,
        ..spec()
    });
    let (_, shop_lease) = environment_lease(&rig, id, "shop");
    let before = rig.instance_row(id).revision();

    let op = rig.stop(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        "supervisor.stop_requires_override"
    );
    let error = detail.view.journal.failure_error().expect("an error");
    let fields: Vec<&str> = error.items()[0]
        .fields()
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();
    assert_eq!(fields, ["environmentId", "name", "alias", "ownership"]);
    assert_eq!(rig.instance_row(id).revision(), before);
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}

// covers: AC-5 (spec 0047), `settle_gone_process` with
// `LeaseCheck::EnvironmentDown` and no other demand
#[tokio::test]
async fn a_down_of_a_stopping_instance_whose_process_is_gone_writes_stopped_and_releases_its_lease()
{
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Stopping,
        desired: DesiredState::Running,
        ..spec()
    });
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    assert!(outcome(&detail).partial.is_empty(), "not kept");
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(row.desired(), DesiredState::Stopped);
    assert!(matches!(
        lease_state(&rig, id, &shop_lease),
        LeaseState::Released { .. }
    ));
}

// covers: AC-5 (spec 0047), the quiet branch of `run_stop` with
// `LeaseCheck::EnvironmentDown`: a stopped instance whose desired is still
// `running` gets desired `stopped`, so the start at login leaves it be
#[tokio::test]
async fn a_down_of_a_stopped_instance_still_wanted_running_writes_desired_stopped_only() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        desired: DesiredState::Running,
        ..spec()
    });
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    assert!(outcome(&detail).partial.is_empty(), "not kept");
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(row.desired(), DesiredState::Stopped);
    assert!(matches!(
        lease_state(&rig, id, &shop_lease),
        LeaseState::Released { .. }
    ));
    assert_eq!(main_spawns(&rig), 0, "nothing was started or signalled");
}

// covers: AC-5 (spec 0047), *Respawn*: with `Committed` the pending respawn
// is cancelled, so the instance stays stopped
#[tokio::test]
async fn a_down_of_a_recovering_instance_only_its_lease_holds_cancels_the_respawn() {
    let rig = slow_respawn_rig().await;
    let id = rig.instance(&spec());
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");
    recovering(&rig, id).await;
    release_all_but(&rig, id, &shop_lease);

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;
    // Longer than the pause before a respawn.
    tokio::time::sleep(Duration::from_millis(2500)).await;

    assert_eq!(outcome(&detail).resources, vec![ResourceRef::Instance(id)]);
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(row.desired(), DesiredState::Stopped);
    assert_eq!(main_spawns(&rig), 1, "no respawn after the Down");
    assert!(matches!(
        lease_state(&rig, id, &shop_lease),
        LeaseState::Released { .. }
    ));
}

// covers: AC-16 (spec 0047), *Respawn*: a Stop All step without the override
// that skips an instance an environment holds leaves its respawn alone
#[tokio::test]
async fn a_recovering_instance_a_stop_all_step_skips_still_gets_its_respawn() {
    let rig = slow_respawn_rig().await;
    let id = rig.instance(&spec());
    let (_, shop_lease) = environment_lease(&rig, id, "shop");
    recovering(&rig, id).await;

    let detail = stop_all_of(&rig, id, StopAllRequest::new(vec![id])).await;

    let result = outcome(&detail);
    assert!(result.resources.is_empty());
    assert_eq!(result.partial.len(), 1);
    assert_eq!(result.partial[0].outcome, PartialOutcome::Skipped);
    assert_eq!(
        result.partial[0].code.as_deref(),
        Some("supervisor.stop_requires_override")
    );
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    wait_for("the respawn runs", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
    })
    .await;
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}

// covers: AC-14 (spec 0047): a start that is cancelled stops the process it
// started itself without reading the lease, and the lease stays
#[tokio::test]
async fn a_cancelled_start_of_an_instance_an_environment_holds_stops_its_own_process() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let (_, shop_lease) = environment_lease(&rig, id, "shop");

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    wait_for("the process runs", || rig.sup.status(id).process.is_some()).await;
    rig.engine
        .cancel(op, stackly_domain::OperationTrigger::background())
        .await
        .expect("the start can be cancelled");
    wait_for("the stop signal arrives", || {
        !rig.processes().signals(spawn.pid).is_empty()
    })
    .await;
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
    let detail = rig.wait_final(op).await;

    assert_eq!(detail.view.operation.state(), OperationState::Cancelled);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}

// covers: AC-14 (spec 0047): a rename needs no override, and a force stop
// of an instance that is already `stopping` (a stop with the override that
// timed out) reads no lease; the lease stays
#[tokio::test]
async fn a_rename_and_a_force_stop_of_an_instance_an_environment_holds_need_no_override() {
    let rig = Rig::patched(|text| replace(text, "stopSeconds: 10", "stopSeconds: 1")).await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let (_, shop_lease) = environment_lease(&rig, id, "shop");

    let rename = rig.rename(id, "renamed-db").await;
    rig.wait_state(rename, OperationState::Succeeded).await;
    assert_eq!(rig.instance_row(id).name().as_str(), "renamed-db");

    let stop = stop_with(&rig, StopRequest::new(id).overriding_leases()).await;
    let timed_out = rig.wait_state(stop, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&timed_out),
        codes::STOP_TIMEOUT.to_string()
    );
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Stopping
    );
    rig.kill_servers();
    let force = rig.force_stop(id).await;
    rig.wait_state(force, OperationState::Succeeded).await;

    assert_eq!(rig.processes().signals(spawn.pid), [ProviderSignal::Term]);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}
