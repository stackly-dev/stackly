//! The rule that chooses what Stop All stops (spec 0031 AC-10, *Pilihan stop
//! all*): one case per row of its table, the order, and the cut down view.

use stackly_core::instance::stop_all::{
    DemandSource, StopAllChanged, StopAllInstance, StopAllView, confirmed_steps, select,
};
use stackly_domain::{
    DesiredState, EnvironmentId, InstanceId, InstanceLifecycle, Lease, LeaseHolder, LeaseId,
    SessionId, StartupPolicy,
};

fn session() -> SessionId {
    "6f9619ff-8b86-4011-b42d-00c04fc964ff/100014"
        .parse()
        .expect("a session ID")
}

fn lease(id: InstanceId, holder: LeaseHolder) -> Lease {
    let session = match holder {
        LeaseHolder::Environment(_) => None,
        _ => Some(session()),
    };
    Lease::new(
        LeaseId::generate(),
        id,
        holder,
        session,
        "2026-11-02T08:00:00Z".parse().expect("a time"),
    )
    .expect("a lease")
}

fn item(name: &str, lifecycle: InstanceLifecycle, desired: DesiredState) -> StopAllInstance {
    let id = InstanceId::generate();
    StopAllInstance {
        id,
        name: name.parse().expect("a name"),
        lifecycle,
        desired,
        startup: StartupPolicy::Manual,
        retained: false,
        leases: Vec::new(),
        open_start: false,
        needs_recovery: false,
    }
}

fn view(instances: Vec<StopAllInstance>) -> StopAllView {
    StopAllView {
        instances,
        session: None,
    }
}

use InstanceLifecycle as L;

#[test]
fn a_retained_instance_is_not_selected_and_an_unknown_one_is_skipped() {
    let mut retained = item("kept", L::Retained, DesiredState::Running);
    retained.retained = true;
    let unknown = item("lost", L::Unknown, DesiredState::Running);
    let selection = select(&view(vec![retained, unknown]));

    assert!(selection.stop.is_empty());
    assert_eq!(selection.skipped.len(), 1);
    assert_eq!(
        selection.skipped[0].code.as_str(),
        "supervisor.identity_unproven"
    );
}

#[test]
fn an_instance_that_does_not_run_and_is_not_wanted_is_not_selected() {
    let idle = item("idle", L::Stopped, DesiredState::Stopped);
    let failed = item("failed", L::Failed, DesiredState::Stopped);
    let selection = select(&view(vec![idle, failed]));

    assert_eq!(selection, Default::default());
}

#[test]
fn an_instance_that_is_wanted_without_a_process_or_runs_is_stopped() {
    let waiting = item("waiting", L::Stopped, DesiredState::Running);
    let running = item("running", L::Running, DesiredState::Running);
    let starting = item("starting", L::Starting, DesiredState::Running);
    let recovering = item("recovering", L::Recovering, DesiredState::Running);
    let selection = select(&view(vec![waiting, running, starting, recovering]));

    let names: Vec<_> = selection
        .stop
        .iter()
        .map(|e| e.name.as_str().to_owned())
        .collect();
    assert_eq!(names, ["recovering", "running", "starting", "waiting"]);
}

#[test]
fn a_restart_in_its_stop_half_is_stopped_and_a_start_that_is_queued_counts() {
    // The stop half of a restart writes desired `stopped`.
    let mut restarting = item("restarting", L::Stopping, DesiredState::Stopped);
    restarting.open_start = true;
    // A user start that waits has not written desired yet.
    let mut queued = item("queued", L::Stopped, DesiredState::Stopped);
    queued.open_start = true;
    let selection = select(&view(vec![restarting, queued]));

    assert_eq!(selection.stop.len(), 2);
    assert!(selection.already_stopping.is_empty());
}

#[test]
fn a_stopping_instance_without_an_open_start_is_already_stopping() {
    let stopping = item("stopping", L::Stopping, DesiredState::Stopped);
    let selection = select(&view(vec![stopping]));

    assert!(selection.stop.is_empty());
    assert_eq!(selection.already_stopping.len(), 1);
}

#[test]
fn an_operation_that_needs_recovery_skips_the_instance() {
    let mut held = item("held", L::Running, DesiredState::Running);
    held.needs_recovery = true;
    let mut idle = item("idle", L::Stopped, DesiredState::Stopped);
    idle.needs_recovery = true;
    let selection = select(&view(vec![held, idle]));

    assert_eq!(
        selection.skipped.len(),
        1,
        "an idle instance is not selected at all"
    );
    assert_eq!(
        selection.skipped[0].code.as_str(),
        "operation.recovery_required"
    );
}

#[test]
fn an_active_environment_lease_skips_the_instance_with_the_override_code() {
    let mut held = item("held", L::Running, DesiredState::Running);
    held.leases.push(lease(
        held.id,
        LeaseHolder::Environment(EnvironmentId::generate()),
    ));
    let selection = select(&view(vec![held]));

    assert!(selection.stop.is_empty());
    assert_eq!(
        selection.skipped[0].code.as_str(),
        "supervisor.stop_requires_override"
    );
}

#[test]
fn the_entry_names_its_demand_and_the_startup_lease_of_the_session() {
    let mut running = item("running", L::Running, DesiredState::Running);
    running.startup = StartupPolicy::AtLogin;
    running.leases.push(lease(running.id, LeaseHolder::Manual));
    running.leases.push(lease(running.id, LeaseHolder::Startup));
    let mut selection_view = view(vec![running]);
    selection_view.session = Some(session());
    let selection = select(&selection_view);

    let entry = &selection.stop[0];
    assert_eq!(entry.demand, [DemandSource::Manual, DemandSource::Startup]);
    assert_eq!(
        entry.session_lease,
        stackly_core::session::StartupLeaseState::Active
    );
}

#[test]
fn every_list_is_ordered_by_name_and_then_id() {
    let b = item("b", L::Running, DesiredState::Running);
    let a = item("a", L::Running, DesiredState::Running);
    let selection = select(&view(vec![b, a]));

    assert_eq!(selection.stop[0].name.as_str(), "a");
}

#[test]
fn the_view_of_one_instance_holds_only_that_instance() {
    let a = item("a", L::Running, DesiredState::Running);
    let b = item("b", L::Running, DesiredState::Running);
    let id = a.id;
    let both = view(vec![a, b]);

    let one = both.of_instance(id).expect("the instance is in the view");

    assert_eq!(one.instances.len(), 1);
    assert!(both.of_instance(InstanceId::generate()).is_none());
}

#[test]
fn without_an_expected_set_every_selected_instance_is_a_step() {
    let a = item("a", L::Running, DesiredState::Running);
    let b = item("b", L::Running, DesiredState::Running);
    let ids = [a.id, b.id];
    let selection = select(&view(vec![b, a]));

    let steps = confirmed_steps(&selection, None).expect("no expectation never changes");

    assert_eq!(steps, ids);
}

#[test]
fn the_expected_set_in_any_order_with_repeats_gives_the_selected_steps_by_name() {
    let a = item("a", L::Running, DesiredState::Running);
    let b = item("b", L::Running, DesiredState::Running);
    let (a_id, b_id) = (a.id, b.id);
    let selection = select(&view(vec![a, b]));

    let steps =
        confirmed_steps(&selection, Some(&[b_id, a_id, b_id])).expect("the same set confirms");

    assert_eq!(steps, [a_id, b_id]);
}

#[test]
fn an_instance_that_joined_the_selection_since_the_preview_changes_it() {
    let a = item("a", L::Running, DesiredState::Running);
    let late = item("late", L::Starting, DesiredState::Running);
    let a_id = a.id;
    let selection = select(&view(vec![a, late]));

    assert_eq!(
        confirmed_steps(&selection, Some(&[a_id])),
        Err(StopAllChanged)
    );
}

#[test]
fn an_instance_that_left_the_selection_since_the_preview_changes_it() {
    let a = item("a", L::Running, DesiredState::Running);
    let gone = InstanceId::generate();
    let a_id = a.id;
    let selection = select(&view(vec![a]));

    assert_eq!(
        confirmed_steps(&selection, Some(&[a_id, gone])),
        Err(StopAllChanged)
    );
}

#[test]
fn an_instance_that_began_stopping_on_its_own_is_not_in_the_selected_set() {
    let a = item("a", L::Running, DesiredState::Running);
    let stopping = item("b", L::Stopping, DesiredState::Stopped);
    let (a_id, stopping_id) = (a.id, stopping.id);
    let selection = select(&view(vec![a, stopping]));

    assert_eq!(confirmed_steps(&selection, Some(&[a_id])), Ok(vec![a_id]));
    assert_eq!(
        confirmed_steps(&selection, Some(&[a_id, stopping_id])),
        Err(StopAllChanged)
    );
}

#[test]
fn an_empty_expected_set_matches_an_empty_selection_only() {
    let idle = item("idle", L::Stopped, DesiredState::Stopped);
    let empty = select(&view(vec![idle]));
    let running = select(&view(vec![item("a", L::Running, DesiredState::Running)]));

    assert_eq!(confirmed_steps(&empty, Some(&[])), Ok(Vec::new()));
    assert_eq!(confirmed_steps(&running, Some(&[])), Err(StopAllChanged));
}
