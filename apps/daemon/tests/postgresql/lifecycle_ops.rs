//! Stop, restart, and reload of a real PostgreSQL 18 through the contract
//! methods of spec 0026 (AC-1, AC-6, AC-10, AC-11): the stop strategy with a
//! session open ends clean, a restart runs a new postmaster on the same data,
//! and `instance.reload` keeps the postmaster.

use std::process::{Command, Stdio};
use std::time::Duration;

use stackly_protocol::instance::{InstanceLastExit, InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG18, Site, create_instance, stop_and_shutdown};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_stops_clean_restarts_with_its_data_and_reloads_with_the_same_pid() {
    let ev = evidence!(
        covers = ["REQ-011", "AC-011", "AC-026", "BR-016"],
        kind = Kind::Native,
        expected = "a PostgreSQL 18 instance with a session open stops through instance.stop with lastExit clean; instance.restart runs a new postmaster that still holds the sentinel row and reads lastExit clean; instance.reload keeps the postmaster pid, is ready again, and the summary says capabilities.reload true",
    );
    let Some(site) = Site::open(&ev, PG18) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "lifecycle-ops").await;
    let running = api.start_ready(made.id).await;
    assert!(running.capabilities.reload);
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    let written = psql.scalar(
        &made.password,
        "CREATE TABLE sentinel (v text); INSERT INTO sentinel VALUES ('kept'); SELECT 1",
    );
    assert!(written.ends_with("INSERT 0 1\n1"), "{written}");

    // AC-1: a stop with a session open is the provider's strategy and ends clean.
    let run = site.run_dir(made.id);
    let mut session = Command::new(site.bin("psql"))
        .args([
            "--no-psqlrc",
            "--no-password",
            &format!("--host={}", run.display()),
            &format!("--port={}", made.port),
            "--username=postgres",
            "--dbname=postgres",
            "--command=SELECT pg_sleep(600)",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PGPASSWORD", &made.password)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("psql starts the long query");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let stop = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(stop.operation.id, OperationState::Succeeded)
        .await;
    let stopped = api.get(made.id).await.expect("instance.get");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(stopped.last_exit, Some(InstanceLastExit::Clean));
    let _ = session.kill();
    let _ = session.wait();

    // AC-6: a restart is one operation, a new postmaster, the same data.
    let first = api
        .start_ready(made.id)
        .await
        .process
        .expect("a process")
        .pid;
    let restart = api
        .restart(made.id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("an operation");
    api.wait_state(restart.operation.id, OperationState::Succeeded)
        .await;
    let restarted = api
        .wait_instance(made.id, "ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    let second = restarted.process.as_ref().expect("a process").pid;
    assert_ne!(second, first, "a restart runs a new postmaster");
    assert_eq!(restarted.last_exit, Some(InstanceLastExit::Clean));
    assert_eq!(
        psql.scalar(&made.password, "SELECT v FROM sentinel"),
        "kept",
        "the data survives the restart"
    );

    // AC-10: a reload keeps the postmaster and is ready again.
    let reload = api
        .reload(made.id)
        .await
        .expect("the reload is accepted")
        .accepted
        .expect("an operation");
    api.wait_state(reload.operation.id, OperationState::Succeeded)
        .await;
    let reloaded = api.get(made.id).await.expect("instance.get");
    assert_eq!(reloaded.process.as_ref().map(|p| p.pid), Some(second));
    assert_eq!(reloaded.readiness.state, InstanceReadinessState::Ready);

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version}: stop clean with a session open, restart {first} -> {second} with the sentinel kept, reload kept {second}"
    ));
}
