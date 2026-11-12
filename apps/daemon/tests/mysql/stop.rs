//! A stop with `SIGTERM` that does not wait for clients (spec 0042 AC-8).

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{MY84, My, ROOT, Site, create_instance, stop_and_shutdown};
use crate::support::LogRecorder;

async fn stops_fast_with_a_sleeping_session_and_starts_again_without_crash_recovery(
    my: My,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, my) else {
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
    let run = site.run_dir(made.id);
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let mut sleeper = Command::new(site.bin("mysql"))
        .args([
            "--no-defaults".to_owned(),
            "--protocol=socket".to_owned(),
            format!("--socket={}", mysql.socket().display()),
            format!("--user={ROOT}"),
            "--execute=SELECT SLEEP(600)".to_owned(),
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", site.root.path())
        .env("MYSQL_PWD", &made.password)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("mysql starts the long query");
    let mut waiting = false;
    for _ in 0..200 {
        waiting = mysql.scalar(
            &made.password,
            "SELECT COUNT(*) FROM information_schema.processlist WHERE info = 'SELECT SLEEP(600)'",
        ) == "1";
        if waiting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(waiting, "the session runs SELECT SLEEP(600)");

    let started = Instant::now();
    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let took = started.elapsed();
    assert!(took < Duration::from_secs(30), "the stop took {took:?}");
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    let _ = sleeper.kill();
    let _ = sleeper.wait();

    for gone in ["mysqld.pid", "mysql.sock", "mysql.sock.lock"] {
        assert!(!run.join(gone).exists(), "{gone} is left");
    }
    let lines = recorder.text_of(made.id);
    assert!(
        lines.iter().any(|line| line.contains("Shutdown complete")),
        "{lines:#?}"
    );

    let before = lines.len();
    api.start_ready(made.id).await;
    let lines = recorder.text_of(made.id);
    let second: Vec<&String> = lines.iter().skip(before).collect();
    assert!(!second.is_empty(), "the second start logged");
    assert!(
        second
            .iter()
            .all(|line| !line.contains("Database was not shutdown normally")),
        "{second:#?}"
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "stopped in {} ms with SLEEP(600) open, Shutdown complete, no crash recovery on the next start",
        took.as_millis()
    ));
}

// covers: AC-8 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_stops_fast_with_a_sleeping_session_and_starts_again_without_crash_recovery() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "with one session in SELECT SLEEP(600), the SIGTERM stop ends stopped within 30 s, the pid file, socket, and socket lock file are gone, the log says Shutdown complete, and the next start does not log Database was not shutdown normally",
    );
    stops_fast_with_a_sleeping_session_and_starts_again_without_crash_recovery(MY84, ev).await;
}
