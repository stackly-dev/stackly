//! `execute_init` on the fake provider (spec 0010 AC-13, AC-15): secrets
//! resolved first, the stdin secret, the lock at fd 3, the cleanup, the
//! failure reasons, and the lock released before an error returns.

use std::sync::Arc;
use std::time::Duration;

use stackly_core::secret::delivery::stdin_payload;
use stackly_core::supervisor::{InitRequest, SupervisorError};
use stackly_domain::{InstanceLifecycle, Registry};
use stackly_platform::{ChildRole, ExitStatus, LockState, ProcessSupervision};
use stackly_testkit::ProcessOp;

use crate::common::{Rig, Spec, TOKEN, config_file, replace};

fn uninitialized() -> Spec {
    Spec {
        lifecycle: InstanceLifecycle::Uninitialized,
        ..Spec::default()
    }
}

#[tokio::test]
async fn init_runs_the_plan_with_the_secret_on_stdin_and_the_lock_at_fd_three() {
    let rig = Arc::new(Rig::new().await);
    let id = rig.instance(&uninitialized());
    let init = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move {
            rig.sup
                .execute_init(InitRequest::new(id, Rig::user()))
                .await
        })
    };

    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;
    let data = rig.paths(id).data;
    assert_eq!(
        step.args,
        [
            "init".to_owned(),
            format!("--data={}", data.display()),
            "--token-stdin".to_owned(),
            "--fail=false".to_owned(),
            "--delay-ms=0".to_owned(),
        ]
    );
    assert_eq!(step.stdin_len, Some(TOKEN.len()));
    let stdin = rig
        .processes()
        .take_stdin(step.pid)
        .expect("a stdin secret");
    assert_eq!(stdin_payload(&stdin), TOKEN.as_bytes());
    assert!(step.lock.is_some(), "an init step inherits the lock");
    // The instance lock is held while the step runs.
    assert_eq!(
        rig.processes().probe_lock(&rig.paths(id).lock).unwrap(),
        LockState::Held
    );
    rig.processes().exit(step.pid, Some(ExitStatus::Exited(0)));

    init.await.unwrap().expect("the init succeeds");

    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Stopped);
    assert!(instance.data_initialized_at().is_some());
    assert!(
        config_file(&rig.paths(id)).is_file(),
        "the config is written"
    );
    let manifest = stackly_core::read_instance_manifest(&rig.platform, &rig.paths(id)).unwrap();
    assert_eq!(
        manifest.data().initialized_at(),
        instance.data_initialized_at()
    );
    rig.wait_lock_free(id).await;
}

#[tokio::test]
async fn a_step_that_exits_with_another_code_fails_the_init_and_frees_the_lock() {
    let rig = Arc::new(Rig::new().await);
    let id = rig.instance(&uninitialized());
    let init = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move {
            rig.sup
                .execute_init(InitRequest::new(id, Rig::user()))
                .await
        })
    };
    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;
    rig.processes().exit(step.pid, Some(ExitStatus::Exited(3)));

    let error = init.await.unwrap().expect_err("the step failed");

    match error {
        SupervisorError::InitFailed {
            reason, exit_code, ..
        } => {
            assert_eq!(reason, "init_step_failed");
            assert_eq!(exit_code, Some(3));
        }
        other => panic!("{other:?}"),
    }
    let instance = rig.instance_row(id);
    assert_eq!(instance.lifecycle(), InstanceLifecycle::Failed);
    assert_eq!(
        instance.last_failure().map(|code| code.as_str()),
        Some("init_step_failed")
    );
    // K10: the executor released the lock before the error came back.
    rig.wait_lock_free(id).await;
}

#[tokio::test]
async fn a_step_past_its_timeout_gets_term_then_kill_and_fails_with_init_step_timeout() {
    let rig = Arc::new(
        Rig::patched(|text| {
            replace(
                text,
                "      stdin: { secret: authToken }",
                "      stdin: { secret: authToken }\n      timeoutSeconds: 1",
            )
        })
        .await,
    );
    let id = rig.instance(&uninitialized());
    let init = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move {
            rig.sup
                .execute_init(InitRequest::new(id, Rig::user()))
                .await
        })
    };
    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let forces: Vec<_> = rig
            .processes()
            .calls()
            .into_iter()
            .filter(|call| call.op == ProcessOp::TerminateHelperGroup)
            .map(|call| call.force)
            .collect();
        if forces == [Some(false), Some(true)] {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{forces:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    rig.processes()
        .exit(step.pid, Some(ExitStatus::Signaled(9)));

    let error = init.await.unwrap().expect_err("the step timed out");

    assert!(
        matches!(&error, SupervisorError::InitFailed { reason, .. } if reason == "init_step_timeout"),
        "{error:?}"
    );
    assert_eq!(
        rig.instance_row(id)
            .last_failure()
            .map(|code| code.as_str()),
        Some("init_step_timeout")
    );
}

#[tokio::test]
async fn a_missing_secret_stops_the_init_before_any_side_effect() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        token: None,
        ..uninitialized()
    });

    let error = rig
        .sup
        .execute_init(InitRequest::new(id, Rig::user()))
        .await
        .expect_err("the secret is missing");

    assert!(matches!(error, SupervisorError::Secret(_)), "{error:?}");
    assert_eq!(
        rig.instance_row(id).lifecycle(),
        InstanceLifecycle::Uninitialized
    );
    assert!(rig.processes().spawns().is_empty(), "no step ran");
    assert!(rig.sup.status(id).blocker.is_some(), "the blocker is set");
    assert!(
        !config_file(&rig.paths(id)).exists(),
        "no config was written"
    );
    let _ = rig.registry.instance(id);
}

#[tokio::test]
async fn an_init_that_runs_past_its_deadline_fails_with_init_timeout() {
    let rig =
        Arc::new(Rig::patched(|text| replace(text, "initSeconds: 30", "initSeconds: 1")).await);
    let id = rig.instance(&uninitialized());
    let init = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move {
            rig.sup
                .execute_init(InitRequest::new(id, Rig::user()))
                .await
        })
    };
    let step = rig.helper_spawn(ChildRole::InitStep, 0).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while rig.processes().count(ProcessOp::TerminateHelperGroup) < 2 {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    rig.processes()
        .exit(step.pid, Some(ExitStatus::Signaled(9)));

    let error = init.await.unwrap().expect_err("the deadline passed");

    assert!(
        matches!(&error, SupervisorError::InitFailed { reason, .. } if reason == "init_timeout"),
        "{error:?}"
    );
    assert_eq!(
        rig.instance_row(id)
            .last_failure()
            .map(|code| code.as_str()),
        Some("init_timeout")
    );
}
