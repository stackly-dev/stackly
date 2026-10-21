//! The launch checks of spec 0018 in the start of an instance (spec 0010,
//! K7): a quarantined executable is refused before the spawn, and a child
//! that macOS kills before it is ready is a binary the OS rejected, with no
//! restart.

use stackly_domain::{InstanceLifecycle, OperationState};
use stackly_platform::{ChildRole, ExitStatus};
use stackly_testkit::ProcessOp;

use crate::common::{Rig, Spec};

fn main_spawns(rig: &Rig) -> usize {
    rig.processes()
        .spawns()
        .into_iter()
        .filter(|spawn| spawn.role == ChildRole::Main)
        .count()
}

#[tokio::test]
async fn a_quarantined_executable_refuses_the_start_before_the_spool_and_the_spawn() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    rig.fake
        .code_signing()
        .set_quarantined("stackly-fake-server", Some("Safari"));

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), "runtime.quarantined");
    assert_eq!(main_spawns(&rig), 0, "nothing was spawned");
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        instance.last_failure().map(|code| code.as_str()),
        Some("quarantined")
    );
}

#[tokio::test]
async fn a_main_process_the_os_killed_before_ready_fails_without_using_the_budget() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Signaled(9)));
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), "runtime.binary_rejected_by_os");
    let details: Vec<_> = detail
        .view
        .journal
        .failure_error()
        .expect("the failed operation stored its error")
        .details()
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect();
    assert!(
        details.contains(&("signal".to_owned(), "9".to_owned())),
        "{details:?}"
    );
    assert_eq!(main_spawns(&rig), 1, "no restart follows");
    assert_eq!(rig.sup.status(id).restart.attempts, 0);
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        instance.last_failure().map(|code| code.as_str()),
        Some("binary_rejected_by_os")
    );
}

#[tokio::test]
async fn another_signal_is_still_an_ordinary_crash_with_a_restart() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let _op = rig.start(id).await;
    let first = rig.main_spawn(1).await;
    rig.processes()
        .exit(first.pid, Some(ExitStatus::Signaled(6)));
    let second = rig.main_spawn(2).await;

    assert_ne!(first.pid, second.pid);
    assert_eq!(main_spawns(&rig), 2);
}

#[tokio::test]
async fn a_child_that_died_of_sigkill_while_suspended_is_a_binary_the_os_rejected() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    rig.processes()
        .die_suspended_on_next_spawn(ExitStatus::Signaled(9));
    rig.registry.refuse_process_observations(true);

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), "runtime.binary_rejected_by_os");
    assert_eq!(rig.processes().count(ProcessOp::Resume), 0, "it never ran");
    assert_eq!(
        rig.instance_row(id)
            .last_failure()
            .map(|code| code.as_str()),
        Some("binary_rejected_by_os")
    );
}

#[tokio::test]
async fn a_discard_the_daemon_does_by_itself_is_never_classified() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    rig.registry.refuse_process_observations(true);

    let op = rig.start(id).await;
    let detail = rig.wait_state(op, OperationState::Failed).await;

    // The daemon killed the suspended child itself: that is not the OS.
    assert_eq!(
        Rig::failure_code(&detail),
        stackly_core::supervisor::codes::IDENTITY_CAPTURE_FAILED.to_string()
    );
}
