//! A crash loop under the production constants of spec 0010, on the
//! `stacklyd` binary (spec 0033 AC-1 and AC-14, milestone 5): no shortened
//! `DaemonConfig` and no paused clock, so the five pauses take 1, 2, 4, 8,
//! and 16 seconds times a jitter of 0.5 to 1.0 and the loop about 20 to 35
//! seconds. The fake crashes before it is ready, so all six spawns belong to
//! the one start the test sends.
//!
//! The evidence comes from `cargo xtask evidence run`, the run that also
//! holds the spec's own bounds on time (the whole loop within 60 seconds,
//! the scenario of spec 0010). A plain `cargo test` shares this Mac with
//! every other test binary, so it holds looser ones; the bounds that prove
//! the budget, the pauses, one process at a time, and no busy loop are the
//! same in both.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_core::supervisor::{
    RESTART_BASE_DELAY, RESTART_BUDGET, RESTART_MAX_DELAY, RESTART_WINDOW,
};
use stackly_protocol::instance::InstanceLifecycle;
use stackly_protocol::operation::{OperationDetail, OperationState};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{EVIDENCE_DIR_ENV, Kind, evidence};
use uuid::Uuid;

use crate::common::{client, daemon_paths, runtime, start_daemon};
use crate::crash_loop::{error_details, markers, started_operation, untouched_at};
use crate::support::{Api, reap_guard, seed_root, serving};

/// The full pause before each of the five respawns, `BASE × 2^(n−1)`, as
/// spec 0010 AC-7 writes them.
const FULL_PAUSES: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
];
/// The exit code of every crash of the fake.
const EXIT_CODE: i32 = 3;
/// How long the fake runs before each crash: long enough for the process
/// sampler to see it, far shorter than its readiness delay.
const CRASH_AFTER_MS: u64 = 500;
/// The scenario bound of spec 0010: the whole crash loop ends in 60 s.
const LOOP_LIMIT: Duration = Duration::from_secs(60);
/// What the pause between an exit and the next spawn may add to its full
/// backoff in an evidence run: noticing the exit, the commit, the lock
/// stage, and the spawn.
const PAUSE_SLACK: Duration = Duration::from_secs(1);
/// The share of the loop's wall time the daemon may spend on the CPU. A
/// pause that spins instead of sleeping keeps one core busy, a share near
/// 1.0.
const CPU_SHARE_LIMIT: f64 = 0.25;
/// How long the test watches for a spawn after the budget ran out: longer
/// than the first pause, the shortest one a respawn would wait.
const QUIET_AFTER: Duration = Duration::from_secs(3);
/// How often the sampler lists the fake's processes.
const SAMPLE_EVERY: Duration = Duration::from_millis(100);

/// Whether this is an evidence run (`cargo xtask evidence run`).
fn evidence_run() -> bool {
    std::env::var_os(EVIDENCE_DIR_ENV).is_some()
}

/// The bound of a timing: the spec's own in an evidence run, `loose` in a
/// plain `cargo test`.
fn bound(spec: Duration, loose: Duration) -> Duration {
    if evidence_run() { spec } else { loose }
}

/// The CPU time, user and system, the process `pid` used so far, from
/// `ps -o time=` (`[[hh:]mm:]ss.ss`).
fn cpu_time(pid: u32) -> Duration {
    let output = std::process::Command::new("/bin/ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let seconds = text.split(':').try_fold(0.0_f64, |total, part| {
        part.trim().parse::<f64>().map(|value| total * 60.0 + value)
    });
    Duration::from_secs_f64(seconds.unwrap_or_else(|_| panic!("ps time of {pid}: {text:?}")))
}

/// What the process sampler saw: the most processes of the fake at once,
/// and how many samples saw one.
#[derive(Debug, Clone, Copy)]
struct Sampled {
    most: usize,
    with_one: usize,
    samples: usize,
}

/// Lists the fake's processes for `data` every [`SAMPLE_EVERY`] until
/// `done` is set.
fn sample_processes(data: std::path::PathBuf, done: Arc<AtomicBool>) -> Sampled {
    let mut sampled = Sampled {
        most: 0,
        with_one: 0,
        samples: 0,
    };
    while !done.load(Ordering::SeqCst) {
        let running = serving(&data).len();
        sampled.most = sampled.most.max(running);
        sampled.with_one += usize::from(running == 1);
        sampled.samples += 1;
        std::thread::sleep(SAMPLE_EVERY);
    }
    sampled
}

/// Waits until the operation ended, longer than the 30 s of
/// [`Api::wait_final`], which the production pauses can pass.
async fn wait_final_within(api: &Api, id: Uuid, limit: Duration) -> OperationDetail {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        let detail = api.operation(id).await;
        if detail.operation.state.is_final() {
            return detail;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "operation {id} did not end within {limit:?}: {detail:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Checks the run markers of six spawns: they alternate `started` and
/// `exited`, so no two main processes ever ran at once and no spawn came
/// during a pause, and each pause from an exit to the next spawn lies
/// between half its full backoff (the smallest jitter, AC-1) and the full
/// backoff plus `slack`. Returns the pauses.
fn pauses_in_turn(markers: &[(bool, jiff::Timestamp)], slack: Duration) -> Vec<Duration> {
    let kinds: Vec<bool> = markers.iter().map(|(started, _)| *started).collect();
    let expected: Vec<bool> = (0..=FULL_PAUSES.len())
        .flat_map(|_| [true, false])
        .collect();
    assert_eq!(
        kinds, expected,
        "every exited comes before the next started"
    );
    FULL_PAUSES
        .iter()
        .enumerate()
        .map(|(index, full)| {
            let exited = markers[2 * index + 1].1;
            let started = markers[2 * index + 2].1;
            let pause = Duration::try_from(started.duration_since(exited))
                .expect("a spawn comes after the exit before it");
            // The markers keep milliseconds, so allow one for the rounding.
            let shortest = (*full / 2).saturating_sub(Duration::from_millis(1));
            let longest = *full + slack;
            assert!(
                pause >= shortest && pause <= longest,
                "respawn {} came {pause:?} after the exit, outside {shortest:?} to {longest:?}",
                index + 1
            );
            pause
        })
        .collect()
}

#[test]
fn the_production_constants_stop_a_crash_loop_at_six_spawns_with_real_pauses() {
    // covers: spec 0033 AC-1, AC-14 (the production constants of spec 0010)
    let ev = evidence!(
        covers = ["AC-068"],
        kind = Kind::Fixture,
        expected = "on the stacklyd binary with the production constants of spec 0010 (no shortened config, no paused clock), a start of a fake that crashes 500 ms after each spawn, before ready, spawns it six times: every exit comes before the next start, the five pauses lie between half and all of 1, 2, 4, 8, and 16 s (plus 1 s to notice and spawn), never two processes of it at once, no spawn after the budget, the daemon on the CPU for under a quarter of the loop, and the loop over within 60 s; it ends failed with crash_loop, an exhausted budget of 5 in 300 s, exit code 3, and a start failed with supervisor.retry_budget_exhausted, attempts 6, lastExitCode 3, and its operation ID, and no port, config revision, data, lock, or run folder name changed",
    );
    assert_eq!(RESTART_BASE_DELAY, FULL_PAUSES[0], "spec 0010 BASE");
    assert_eq!(RESTART_MAX_DELAY, Duration::from_secs(30), "spec 0010 MAX");
    assert_eq!(RESTART_BUDGET, 5, "spec 0010 budget");
    assert_eq!(RESTART_WINDOW, Duration::from_secs(300), "spec 0010 window");
    for (index, full) in FULL_PAUSES.iter().enumerate() {
        let doubled = RESTART_BASE_DELAY * 2_u32.pow(u32::try_from(index).expect("a small count"));
        assert_eq!(
            *full,
            doubled.min(RESTART_MAX_DELAY),
            "respawn {}",
            index + 1
        );
    }

    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let runtime_id = seed_root(&root);
    let paths = daemon_paths(&root);
    let rt = runtime();
    let daemon = start_daemon(&root);
    let pid = daemon.pid();

    let observed = rt.block_on(async {
        let (connected, _) = client(daemon.socket()).await;
        let api = Api::new(connected, runtime_id);
        let id = api
            .create_ready(
                "crashy",
                json!({
                    "crashAfterMs": CRASH_AFTER_MS,
                    "crashExitCode": EXIT_CODE,
                    "readyDelayMs": 60_000,
                }),
            )
            .await;
        let instance = paths
            .layout
            .instance_paths(id.to_string().parse().expect("an instance id"));
        let before = untouched_at(&instance, &api, id).await;

        let done = Arc::new(AtomicBool::new(false));
        let sampler = {
            let data = instance.data.clone();
            let done = Arc::clone(&done);
            std::thread::spawn(move || sample_processes(data, done))
        };
        let cpu_before = cpu_time(pid);
        let began = Instant::now();
        let operation = started_operation(&api, id).await;
        let detail = wait_final_within(&api, operation, bound(LOOP_LIMIT, LOOP_LIMIT * 3)).await;
        let took = began.elapsed();
        let cpu = cpu_time(pid).saturating_sub(cpu_before);

        // The budget: six spawns, then the start fails and the loop stops.
        assert_eq!(detail.operation.state, OperationState::Failed, "{detail:?}");
        assert_eq!(
            Api::failure_code(&detail),
            "supervisor.retry_budget_exhausted"
        );
        let error = &detail.operation.failure.as_ref().expect("a failure").error;
        assert_eq!(error.operation_id(), Some(operation), "the correlation ID");
        let details = error_details(&detail);
        assert!(
            details.contains(&("attempts".to_owned(), "6".to_owned())),
            "{details:?}"
        );
        assert!(
            details.contains(&("lastExitCode".to_owned(), EXIT_CODE.to_string())),
            "{details:?}"
        );
        let failed = api.get(id).await.expect("the instance reads");
        assert_eq!(failed.lifecycle, InstanceLifecycle::Failed);
        assert!(failed.process.is_none());
        assert!(failed.restart.exhausted);
        assert_eq!(failed.restart.budget, RESTART_BUDGET);
        assert_eq!(
            u64::from(failed.restart.window_seconds),
            RESTART_WINDOW.as_secs()
        );
        assert_eq!(failed.restart.next_attempt_at, None, "no respawn is due");
        let failure = failed.last_failure.clone().expect("a failed instance says why");
        assert_eq!(failure.code, "crash_loop");
        assert_eq!(failure.exit_code, Some(EXIT_CODE));

        // The pauses, from the durable log markers of spec 0027.
        let runs = markers(&api, id).await;
        assert_eq!(runs.len(), 12, "six started and six exited: {runs:?}");
        let pauses = pauses_in_turn(&runs, bound(PAUSE_SLACK, PAUSE_SLACK * 10));
        assert!(
            took <= bound(LOOP_LIMIT, LOOP_LIMIT * 3),
            "the crash loop took {took:?}"
        );

        // Nothing after the budget: no spawn, no process.
        tokio::time::sleep(QUIET_AFTER).await;
        assert_eq!(markers(&api, id).await.len(), 12, "no spawn after the budget");
        done.store(true, Ordering::SeqCst);
        let sampled = sampler.join().expect("the sampler thread ends");
        assert!(serving(&instance.data).is_empty(), "no process is left");
        assert_eq!(sampled.most, 1, "never two processes at once: {sampled:?}");
        assert!(sampled.with_one > 0, "the sampler saw the fake run: {sampled:?}");

        // No busy loop: the daemon slept through the pauses.
        let share = cpu.as_secs_f64() / took.as_secs_f64();
        assert!(
            share < CPU_SHARE_LIMIT,
            "the daemon was on the CPU {cpu:?} of {took:?}"
        );

        // AC-14: the crash loop repaired nothing.
        assert_eq!(untouched_at(&instance, &api, id).await, before);
        format!(
            "6 spawns; pauses {} ms after exits (full 1, 2, 4, 8, 16 s); failed with supervisor.retry_budget_exhausted after {} ms; daemon CPU {} ms ({:.1} %); at most {} fake process in {} samples, {} of them with one; no spawn in the {} s after",
            pauses
                .iter()
                .map(|pause| pause.as_millis().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            took.as_millis(),
            cpu.as_millis(),
            share * 100.0,
            sampled.most,
            sampled.samples,
            sampled.with_one,
            QUIET_AFTER.as_secs(),
        )
    });
    daemon.stop().expect("the daemon stops");
    ev.pass_with(&observed);
}
