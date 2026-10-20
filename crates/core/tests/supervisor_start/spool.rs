//! The spool reading of a running process (spec 0010 step 10, AC-12): the
//! lines reach the sinks while it runs and when it exits, secrets are
//! hidden, a busy sink holds nothing back, and an old spool is read before
//! the next run.

use std::fs;
use std::time::Duration;

use stackly_domain::{InstanceId, InstanceLifecycle, OperationState};
use stackly_platform::{ExitStatus, OutputStream};

use crate::common::{Rig, Spec, TOKEN};

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A rig that reads the spools every 20 ms.
async fn quick_rig() -> Rig {
    Rig::with_config(|config| config.spool_drain_interval = Duration::from_millis(20)).await
}

/// Starts the fake and waits until it is running; returns the process.
async fn running(rig: &Rig, id: InstanceId) -> stackly_testkit::FakeSpawn {
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    spawn
}

#[tokio::test]
async fn lines_written_while_the_process_runs_reach_the_sinks_redacted() {
    let rig = quick_rig().await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;

    rig.processes()
        .write_output(
            spawn.pid,
            OutputStream::Stdout,
            format!("hello\nthe token is {TOKEN} ok\n").as_bytes(),
        )
        .expect("the fake process writes");
    rig.processes()
        .write_output(spawn.pid, OutputStream::Stderr, b"warning: slow disk\n")
        .expect("the fake process writes");

    wait_for("three lines", || rig.logs.lines_of(id).len() >= 3).await;
    let lines = rig.logs.lines_of(id);
    assert!(lines.contains(&"hello".to_owned()));
    assert!(lines.contains(&"the token is **** ok".to_owned()));
    assert!(lines.iter().all(|line| !line.contains(TOKEN)));
    // The failure excerpt lists stderr first, then stdout.
    assert_eq!(
        rig.sup.failure_excerpt(id),
        ["warning: slow disk", "hello", "the token is **** ok"]
    );
    rig.kill_servers();
}

#[tokio::test]
async fn the_exit_reads_what_is_left_including_a_line_without_a_newline() {
    // The first tick runs at once; after it the pump sleeps for a minute,
    // so only the final read at exit can see these lines.
    let rig =
        Rig::with_config(|config| config.spool_drain_interval = Duration::from_secs(60)).await;
    let id = rig.instance(&Spec::default());
    let spawn = running(&rig, id).await;
    rig.processes()
        .write_output(spawn.pid, OutputStream::Stderr, b"fatal: cannot open\nbye")
        .expect("the fake process writes");
    assert!(rig.logs.lines_of(id).is_empty());

    rig.processes()
        .exit(spawn.pid, Some(ExitStatus::Exited(70)));

    wait_for("the exit lifecycle", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Failed
    })
    .await;
    wait_for("the last lines", || rig.logs.lines_of(id).len() == 2).await;
    assert_eq!(rig.logs.lines_of(id), ["fatal: cannot open", "bye"]);
    let last = rig.logs.batches_of(id).pop().expect("a batch");
    assert!(last.lines.last().is_some_and(|line| line.partial));
    assert_eq!(rig.sup.failure_excerpt(id), ["fatal: cannot open", "bye"]);
    rig.kill_servers();
}

#[tokio::test]
async fn a_busy_sink_does_not_hold_the_process_or_its_exit() {
    let rig = quick_rig().await;
    let id = rig.instance(&Spec::default());
    rig.logs.set_busy(true);
    let spawn = running(&rig, id).await;
    rig.processes()
        .write_output(spawn.pid, OutputStream::Stdout, b"noisy\n")
        .expect("the fake process writes");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(rig.logs.lines_of(id).is_empty());

    rig.processes().exit(spawn.pid, Some(ExitStatus::Exited(1)));

    wait_for("the exit lifecycle and the forgotten process", || {
        rig.instance_row(id).lifecycle() == InstanceLifecycle::Failed
            && rig.sup.status(id).process.is_none()
    })
    .await;
    rig.kill_servers();
}

#[tokio::test]
async fn an_old_spool_is_read_into_the_sinks_before_the_next_run_and_the_tail_starts_empty() {
    let rig = quick_rig().await;
    let id = rig.instance(&Spec::default());
    let key = id.short_key();
    let path = rig
        .layout
        .layout()
        .instance_spool(key, OutputStream::Stderr);
    stackly_core::ensure_instance_run_dir(&rig.platform, &rig.layout, key)
        .expect("the run folder can be made");
    fs::write(&path, b"left by a dead run\n").expect("the old spool can be written");

    let _spawn = running(&rig, id).await;

    let batches = rig.logs.batches_of(id);
    assert_eq!(rig.logs.lines_of(id), ["left by a dead run"]);
    assert!(batches[0].resumed, "an old spool is read as a resumed one");
    assert!(
        rig.sup.failure_excerpt(id).is_empty(),
        "the tail of an earlier run is forgotten"
    );
    rig.kill_servers();
}
