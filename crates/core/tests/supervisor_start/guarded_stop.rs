//! The stop with a lease guard (spec 0047, *Stop dengan guard lease*,
//! build step 2): the commit that writes desired `stopped` reads the leases
//! inside its transaction and runs `evaluate_stop`, for a person's stop
//! (BR-015, AC-13) and for the stop an environment that goes down begins
//! (BR-014, AC-5). The daemon's own stops read no lease.

use std::time::Duration;

use stackly_core::operation::{OperationOutcome, PartialOutcome};
use stackly_core::session::StartupLeaseState;
use stackly_core::supervisor::{INSTANCE_IN_USE, StopRequest};
use stackly_domain::{
    AuditAction, AuditQuery, DesiredState, EnvironmentId, InstanceId, InstanceLifecycle, Lease,
    LeaseFilter, LeaseState, OperationId, OperationKind, OperationState, Registry, ResourceRef,
    WriteBatch, WriteOp,
};
use stackly_platform::ExitStatus;
use stackly_testkit::{EnvironmentBuilder, FakeSpawn, LeaseBuilder};

use crate::common::{Rig, Spec};

const SESSION: &str = "6f9619ff-8b86-4011-b42d-00c04fc964ff/100014";

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

/// Starts `id` and waits until it is ready; `nth` is the main spawn count
/// after the start.
async fn running(rig: &Rig, id: InstanceId, nth: usize) -> FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(nth).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    spawn
}

/// Stores an environment named `name` that shares `id`, and its active
/// lease on it; returns the environment and the lease.
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

async fn stop_with(rig: &Rig, request: StopRequest) -> OperationId {
    let id = request.instance;
    let work = rig.sup.stop_work(request);
    rig.begin(OperationKind::StopInstance, id, work).await
}

async fn end_on_term(rig: &Rig, spawn: &FakeSpawn) {
    while rig.processes().signals(spawn.pid).is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    rig.kill_servers();
    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(0)));
}

fn outcome(detail: &stackly_core::operation::OperationDetailView) -> OperationOutcome {
    OperationOutcome::from_payload(detail.view.journal.result().expect("a result"))
        .expect("an outcome")
}

/// The details of the one `operation.finished` audit row of `operation`.
fn finished_details(rig: &Rig, operation: OperationId) -> stackly_domain::SafeDetails {
    let query = AuditQuery {
        operation: Some(operation),
        ..AuditQuery::page(50, None)
    };
    rig.registry
        .audit_events(query)
        .expect("the audit rows read")
        .value
        .items
        .into_iter()
        .find(|row| row.action == AuditAction::OPERATION_FINISHED)
        .expect("a finished row")
        .safe_details
}

/// The `environmentId` of each item of a failed operation's error.
fn holder_items(detail: &stackly_core::operation::OperationDetailView) -> Vec<String> {
    let error = detail.view.journal.failure_error().expect("an error");
    error
        .items()
        .iter()
        .map(|item| {
            assert_eq!(item.kind().as_str(), "environment");
            item.fields()
                .iter()
                .find(|(key, _)| key.as_str() == "environmentId")
                .map(|(_, value)| value.clone())
                .expect("an environmentId field")
        })
        .collect()
}

// covers: AC-5 (spec 0047), PRD AC-022
#[tokio::test]
async fn a_down_keeps_an_instance_another_environment_holds_and_writes_nothing() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");
    let (_, erp_lease) = environment_lease(&rig, id, "erp");
    let before = rig.instance_row(id);

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    let outcome = outcome(&detail);
    assert!(outcome.resources.is_empty(), "nothing stopped");
    assert_eq!(outcome.partial.len(), 1);
    assert_eq!(outcome.partial[0].outcome, PartialOutcome::Skipped);
    assert_eq!(
        outcome.partial[0].code.as_deref(),
        Some(INSTANCE_IN_USE.as_str())
    );
    assert_eq!(outcome.partial[0].resource, Some(ResourceRef::Instance(id)));
    assert!(rig.processes().signals(spawn.pid).is_empty(), "no signal");
    let after = rig.instance_row(id);
    assert_eq!(after.revision(), before.revision(), "nothing written");
    assert_eq!(after.lifecycle(), InstanceLifecycle::Running);
    assert_eq!(after.desired(), DesiredState::Running);
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
    assert_eq!(lease_state(&rig, id, &erp_lease), LeaseState::Active);
    assert_eq!(finished_details(&rig, op).get("kept"), Some("true"));
}

// covers: AC-5 (spec 0047)
#[tokio::test]
async fn a_down_stops_an_instance_only_its_own_lease_holds_and_releases_that_lease() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");
    // The manual lease of the start above holds it too; Down keeps an
    // instance a person started, so this one is released first.
    let manual: Vec<Lease> = rig
        .registry
        .leases(id, LeaseFilter::Unreleased)
        .expect("leases read")
        .value
        .into_iter()
        .filter(|lease| lease.id() != shop_lease.id())
        .collect();
    let now = rig.fake_now();
    let released = manual
        .into_iter()
        .map(|mut lease| {
            lease.release(now).expect("an active lease releases");
            WriteOp::UpdateLease(lease)
        })
        .collect();
    rig.registry
        .commit(WriteBatch::new(released))
        .expect("the manual lease is released");

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    end_on_term(&rig, &spawn).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    let outcome = outcome(&detail);
    assert_eq!(outcome.resources, vec![ResourceRef::Instance(id)]);
    assert!(outcome.partial.is_empty());
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(row.desired(), DesiredState::Stopped);
    assert!(matches!(
        lease_state(&rig, id, &shop_lease),
        LeaseState::Released { .. }
    ));
}

// covers: AC-5 (spec 0047)
#[tokio::test]
async fn a_down_keeps_an_instance_a_person_started() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(
        outcome(&detail).partial[0].code.as_deref(),
        Some(INSTANCE_IN_USE.as_str())
    );
    assert!(rig.processes().signals(spawn.pid).is_empty());
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}

// covers: AC-13 (spec 0047), BR-015
#[tokio::test]
async fn a_persons_stop_of_a_live_instance_an_environment_holds_needs_the_override() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");
    let before = rig.instance_row(id);

    let op = rig.stop(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        "supervisor.stop_requires_override"
    );
    assert_eq!(holder_items(&detail), vec![shop.to_string()]);
    assert!(rig.processes().signals(spawn.pid).is_empty());
    assert_eq!(rig.instance_row(id).revision(), before.revision());
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}

// covers: AC-13 (spec 0047)
#[tokio::test]
async fn a_persons_stop_with_the_override_stops_it_keeps_the_lease_and_notes_the_environment() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let (_, shop_lease) = environment_lease(&rig, id, "shop");

    let op = stop_with(&rig, StopRequest::new(id).overriding_leases()).await;
    end_on_term(&rig, &spawn).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(row.desired(), DesiredState::Stopped);
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
    assert_eq!(
        finished_details(&rig, op).get("overriddenEnvironments"),
        Some("shop")
    );
}

// covers: AC-13, AC-11 (spec 0047)
#[tokio::test]
async fn a_lease_an_up_records_after_the_early_check_still_refuses_the_stop_in_the_commit() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
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
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        "supervisor.stop_requires_override"
    );
    assert_eq!(holder_items(&detail), vec![environment.id().to_string()]);
    assert!(
        rig.processes().signals(spawn.pid).is_empty(),
        "nothing stopped"
    );
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Running);
    assert_eq!(row.desired(), DesiredState::Running);
    assert_eq!(lease_state(&rig, id, &lease), LeaseState::Active);
}

// covers: AC-13 (spec 0047)
#[tokio::test]
async fn a_persons_stop_of_an_instance_that_does_not_run_needs_no_override() {
    let rig = Rig::new().await;
    rig.sup
        .set_session(Some(SESSION.parse().expect("a session ID")));
    let id = rig.instance(&Spec {
        desired: DesiredState::Running,
        ..spec()
    });
    let (_, shop_lease) = environment_lease(&rig, id, "shop");
    let startup = LeaseBuilder::new(id).startup(SESSION).build();
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertLease(startup.clone())]))
        .expect("the startup lease is stored");

    let op = rig.stop(id).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(rig.instance_row(id).desired(), DesiredState::Stopped);
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
    assert!(matches!(
        lease_state(&rig, id, &startup),
        LeaseState::Suppressed { .. }
    ));
    // The guard's own ops publish the startup lease effect.
    let events = rig
        .events
        .startup_leases
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(events, vec![(id, StartupLeaseState::Suppressed)]);
}

// covers: AC-5 (spec 0047)
#[tokio::test]
async fn a_down_of_an_idle_instance_another_lease_holds_keeps_its_desired_state() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        desired: DesiredState::Running,
        ..spec()
    });
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");
    environment_lease(&rig, id, "erp");

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(
        outcome(&detail).partial[0].code.as_deref(),
        Some(INSTANCE_IN_USE.as_str())
    );
    assert_eq!(rig.instance_row(id).desired(), DesiredState::Running);
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}

// covers: AC-5 (spec 0047), *Key invariants* (a lease decision never holds
// back a lifecycle)
#[tokio::test]
async fn a_kept_down_of_a_stopping_instance_whose_process_is_gone_still_ends_stopped() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Stopping,
        desired: DesiredState::Running,
        ..spec()
    });
    let (shop, shop_lease) = environment_lease(&rig, id, "shop");
    environment_lease(&rig, id, "erp");

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    let detail = rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(
        outcome(&detail).partial[0].code.as_deref(),
        Some(INSTANCE_IN_USE.as_str())
    );
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(
        row.desired(),
        DesiredState::Running,
        "desired is not written"
    );
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}

/// Starts `id`, then crashes its process so it waits `recovering` for a
/// respawn the long pause of the rig holds back.
async fn recovering(rig: &Rig, id: InstanceId) -> FakeSpawn {
    let first = running(rig, id, 1).await;
    rig.kill_servers();
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Exited(70)));
    wait_for("the instance recovers", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Recovering
    })
    .await;
    first
}

/// A rig whose pause before a respawn is long enough for a stop to run in
/// it.
async fn slow_respawn_rig() -> Rig {
    Rig::with_config(|config| {
        config.restart_base_delay = Duration::from_millis(1500);
        config.restart_max_delay = Duration::from_millis(1500);
    })
    .await
}

// covers: AC-5 (spec 0047), *Respawn*
#[tokio::test]
async fn a_recovering_instance_a_down_keeps_still_gets_its_respawn() {
    let rig = slow_respawn_rig().await;
    let id = rig.instance(&spec());
    let (shop, _) = environment_lease(&rig, id, "shop");
    environment_lease(&rig, id, "erp");
    recovering(&rig, id).await;

    let op = stop_with(&rig, StopRequest::environment_down(id, shop)).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    wait_for("the respawn runs", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
    })
    .await;
}

// covers: AC-13 (spec 0047), *Respawn*
#[tokio::test]
async fn a_recovering_instance_whose_stop_the_guard_refuses_still_gets_its_respawn() {
    let rig = slow_respawn_rig().await;
    let id = rig.instance(&spec());
    let environment = EnvironmentBuilder::new().name("late").shared(id).build();
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            environment.clone(),
        )]))
        .expect("the environment is stored");
    recovering(&rig, id).await;
    let lease = LeaseBuilder::new(id).environment(environment.id()).build();
    rig.registry
        .before_guarded_commit(WriteBatch::new(vec![WriteOp::InsertLease(lease)]));

    let op = rig.stop(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;
    assert_eq!(
        Rig::failure_code(&detail),
        "supervisor.stop_requires_override"
    );

    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    wait_for("the respawn runs", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Running
    })
    .await;
}

// covers: AC-14 (spec 0047): a restart reads no lease
#[tokio::test]
async fn a_restart_of_an_instance_an_environment_holds_needs_no_override() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let first = running(&rig, id, 1).await;
    let (_, shop_lease) = environment_lease(&rig, id, "shop");

    let op = rig.restart(id).await;
    end_on_term(&rig, &first).await;
    let second = rig.main_spawn(2).await;
    rig.serve(&second);
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert_eq!(lease_state(&rig, id, &shop_lease), LeaseState::Active);
}

// covers: AC-16 (spec 0047): without the override a lease that appears
// after the selection makes the step skipped, never a stop
#[tokio::test]
async fn a_stop_all_step_skips_an_instance_an_environment_took_after_the_selection() {
    use stackly_core::operation::{BeginOutcome, BeginRequest, BusyPolicy, Origin};
    use stackly_core::supervisor::StopAllRequest;
    use stackly_domain::{JournalPayload, LockKey, OperationResource};

    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let environment = EnvironmentBuilder::new().name("late").shared(id).build();
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            environment.clone(),
        )]))
        .expect("the environment is stored");
    let lease = LeaseBuilder::new(id).environment(environment.id()).build();
    rig.registry
        .before_guarded_commit(WriteBatch::new(vec![WriteOp::InsertLease(lease.clone())]));

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
            work: rig.sup.stop_all_work(StopAllRequest::new(vec![id])),
            precheck: None,
        })
        .await
        .expect("the engine accepts the operation");
    let BeginOutcome::Created(view) = begun else {
        panic!("expected a created operation, got {begun:?}");
    };
    let detail = rig
        .wait_state(view.operation.id(), OperationState::Succeeded)
        .await;

    let result = outcome(&detail);
    assert!(result.resources.is_empty());
    assert_eq!(result.partial.len(), 1);
    assert_eq!(result.partial[0].outcome, PartialOutcome::Skipped);
    assert_eq!(
        result.partial[0].code.as_deref(),
        Some("supervisor.stop_requires_override")
    );
    assert!(rig.processes().signals(spawn.pid).is_empty());
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Running);
    assert_eq!(lease_state(&rig, id, &lease), LeaseState::Active);
}

/// Begins a `stop_all_instances` of `id` with `request` and waits until it
/// succeeded.
async fn stop_all_of(
    rig: &Rig,
    id: InstanceId,
    request: stackly_core::supervisor::StopAllRequest,
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

// covers: AC-16, AC-17 (spec 0047): with the override the check before the
// step and the guard of its stop both let the instance an environment holds
// stop, and its lease stays active
#[tokio::test]
async fn a_stop_all_step_with_the_override_stops_an_instance_an_environment_holds() {
    use stackly_core::supervisor::StopAllRequest;

    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let (_, lease) = environment_lease(&rig, id, "shop");

    let stopping = stop_all_of(
        &rig,
        id,
        StopAllRequest::new(vec![id]).overriding_leases(true),
    );
    let (detail, ()) = tokio::join!(stopping, end_on_term(&rig, &spawn));

    let result = outcome(&detail);
    assert_eq!(result.resources, vec![ResourceRef::Instance(id)]);
    assert!(result.partial.is_empty());
    let row = rig.instance_row(id);
    assert_eq!(row.lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(row.desired(), DesiredState::Stopped);
    assert_eq!(lease_state(&rig, id, &lease), LeaseState::Active);
}

// covers: AC-16 (spec 0047): with the override a lease an Up records after
// the selection does not turn the step into a skip
#[tokio::test]
async fn a_stop_all_step_with_the_override_stops_an_instance_an_environment_took_late() {
    use stackly_core::supervisor::StopAllRequest;

    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let spawn = running(&rig, id, 1).await;
    let environment = EnvironmentBuilder::new().name("late").shared(id).build();
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            environment.clone(),
        )]))
        .expect("the environment is stored");
    let lease = LeaseBuilder::new(id).environment(environment.id()).build();
    rig.registry
        .before_guarded_commit(WriteBatch::new(vec![WriteOp::InsertLease(lease.clone())]));

    let stopping = stop_all_of(
        &rig,
        id,
        StopAllRequest::new(vec![id]).overriding_leases(true),
    );
    let (detail, ()) = tokio::join!(stopping, end_on_term(&rig, &spawn));

    assert_eq!(outcome(&detail).resources, vec![ResourceRef::Instance(id)]);
    assert_eq!(rig.instance_row(id).lifecycle(), InstanceLifecycle::Stopped);
    assert_eq!(lease_state(&rig, id, &lease), LeaseState::Active);
}

fn port_change(port: u16) -> stackly_core::instance::config::ConfigChange {
    stackly_core::instance::config::ConfigChange::Endpoints(vec![(
        "http".parse().expect("an endpoint name"),
        stackly_domain::PortRequest::Fixed(stackly_domain::Port::new(port).expect("not 0")),
    )])
}

async fn change_ports(rig: &Rig, id: InstanceId, port: u16, override_leases: bool) -> OperationId {
    let work = rig.sup.change_work(
        stackly_core::supervisor::ChangeRequest::new(
            id,
            stackly_domain::ConfigRevisionNumber::new(1).expect("a revision number"),
            port_change(port),
            Rig::user(),
        )
        .overriding_leases(override_leases),
    );
    rig.begin(OperationKind::ChangeEndpoints, id, work).await
}

// covers: AC-15 (spec 0047): a lease an Up records after the handler's
// precheck still refuses the change in the guard of its commit
#[tokio::test]
async fn a_port_change_meets_a_late_environment_lease_in_its_guard_and_writes_nothing() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let environment = EnvironmentBuilder::new().name("late").shared(id).build();
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertEnvironment(
            environment.clone(),
        )]))
        .expect("the environment is stored");
    let lease = LeaseBuilder::new(id).environment(environment.id()).build();
    rig.registry
        .before_guarded_commit(WriteBatch::new(vec![WriteOp::InsertLease(lease.clone())]));

    let op = change_ports(&rig, id, Rig::free_port(), false).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        "instance.endpoints_require_override"
    );
    assert_eq!(holder_items(&detail), [environment.id().to_string()]);
    assert_eq!(rig.active(id).number.get(), 1, "no revision was written");
    assert_eq!(lease_state(&rig, id, &lease), LeaseState::Active);
}

// covers: AC-15 (spec 0047)
#[tokio::test]
async fn a_port_change_with_the_override_goes_on_keeps_the_lease_and_notes_the_environment() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let (_, lease) = environment_lease(&rig, id, "shop");

    let op = change_ports(&rig, id, Rig::free_port(), true).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(rig.active(id).number.get(), 2);
    assert_eq!(lease_state(&rig, id, &lease), LeaseState::Active);
    assert_eq!(
        finished_details(&rig, op).get("overriddenEnvironments"),
        Some("shop")
    );
}

// covers: AC-14 (spec 0047): a settings change reads no lease
#[tokio::test]
async fn a_settings_change_of_an_instance_an_environment_holds_needs_no_override() {
    let rig = Rig::new().await;
    let id = rig.instance(&spec());
    let (_, lease) = environment_lease(&rig, id, "shop");

    let op = rig.apply(id, serde_json::json!({"label": "held"})).await;
    rig.wait_state(op, OperationState::Succeeded).await;

    assert_eq!(rig.active(id).number.get(), 2);
    assert_eq!(lease_state(&rig, id, &lease), LeaseState::Active);
}
