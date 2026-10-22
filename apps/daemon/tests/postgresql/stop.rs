//! A stop that does not wait for clients (spec 0019 AC-10).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance, stop_and_shutdown};
use crate::support::LogRecorder;
use std::sync::Arc;

async fn stops_fast_with_a_session_open_and_starts_again_without_crash_recovery(pg: Pg) {
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "with SELECT pg_sleep(600) running, instance.stop finishes in 10 seconds, the pid file, the socket, and its lock are gone, the log shows a fast shutdown, and the next start logs a clean shutdown with no crash recovery",
    );
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let recorder = Arc::new(LogRecorder::default());
    let daemon = site
        .in_process(|config| {
            config
                .extra_log_sinks
                .push(Arc::clone(&recorder) as Arc<dyn stackly_core::supervisor::LogSink>);
        })
        .await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "stopper").await;
    api.start_ready(made.id).await;
    let paths = site.instance_paths(made.id);
    let run = site.run_dir(made.id);
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    let mut sleeper = Command::new(site.bin("psql"))
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
    for _ in 0..200 {
        let waiting = psql.scalar(
            &made.password,
            "SELECT count(*) FROM pg_stat_activity WHERE query LIKE 'SELECT pg_sleep(600)%' AND state = 'active'",
        );
        if waiting == "1" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let started = Instant::now();
    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let took = started.elapsed();
    assert!(took < Duration::from_secs(10), "the stop took {took:?}");
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    let _ = sleeper.kill();
    let _ = sleeper.wait();

    assert!(!paths.data.join("postmaster.pid").exists());
    assert!(!run.join(format!(".s.PGSQL.{}", made.port)).exists());
    assert!(!run.join(format!(".s.PGSQL.{}.lock", made.port)).exists());
    let lines = recorder.text_of(made.id);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("received fast shutdown request")),
        "{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("database system is shut down")),
        "{lines:#?}"
    );

    let before = lines.len();
    api.start_ready(made.id).await;
    let lines = recorder.text_of(made.id);
    let second: Vec<&String> = lines.iter().skip(before).collect();
    assert!(
        second
            .iter()
            .any(|l| l.contains("database system was shut down at")),
        "{second:#?}"
    );
    assert!(
        second.iter().all(|l| !l.contains("not properly shut down")),
        "{second:#?}"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "stopped in {took:?} with a 600 second query running"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_stops_fast_with_a_session_open_and_starts_again_without_crash_recovery() {
    stops_fast_with_a_session_open_and_starts_again_without_crash_recovery(PG17).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_stops_fast_with_a_session_open_and_starts_again_without_crash_recovery() {
    stops_fast_with_a_session_open_and_starts_again_without_crash_recovery(PG18).await;
}
