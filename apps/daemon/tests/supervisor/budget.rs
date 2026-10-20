//! The overhead of the daemon with ten idle fake instances (spec 0010 AC-20,
//! NFR-003, supervisor part): the real `stacklyd` binary, ten running and
//! ready instances with their readiness and health probes on, no log spam,
//! sampled for five minutes.
//!
//! The measurement runs only with `STACKLY_MEASURE=1`: it takes more than
//! five minutes and its numbers are recorded by hand with `cargo xtask
//! evidence new`. Run it on the Mac that holds the baseline:
//! `STACKLY_MEASURE=1 cargo test -p stacklyd --test supervisor measure_ -- --nocapture`.

use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_testkit::TestRoot;

use crate::common::{client, runtime, start_daemon};
use crate::support::{Api, reap_guard, seed_root};

/// The most resident memory the daemon may hold, in KiB (150 MiB).
const RSS_MAX_KIB: u64 = 150 * 1024;
/// The most CPU the daemon may use on average, in percent of one logical
/// core.
const CPU_MAX_PERCENT: f64 = 1.0;
/// How many instances run idle.
const INSTANCES: usize = 10;
/// How long the daemon is sampled.
const SAMPLE_FOR: Duration = Duration::from_secs(300);

/// How often it is sampled.
const SAMPLE_EVERY: Duration = Duration::from_secs(5);

/// Resident memory in KiB and CPU time in seconds of `pid`, from `ps`.
fn sample(pid: u32) -> (u64, f64) {
    let output = Command::new("/bin/ps")
        .args(["-o", "rss=,time=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.split_whitespace();
    let rss = parts
        .next()
        .and_then(|rss| rss.parse().ok())
        .expect("ps prints the resident size");
    let time = parts.next().expect("ps prints the cpu time");
    (rss, cpu_seconds(time))
}

/// `[[h:]m:]s.cc` as seconds.
fn cpu_seconds(text: &str) -> f64 {
    text.split(':').fold(0.0, |total, part| {
        total * 60.0 + part.parse::<f64>().unwrap_or(0.0)
    })
}

#[test]
fn measure_the_daemon_with_ten_idle_fake_instances() {
    if std::env::var_os("STACKLY_MEASURE").is_none() {
        eprintln!("skipped: set STACKLY_MEASURE=1 to measure the daemon for five minutes");
        return;
    }
    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let runtime_id = seed_root(&root);
    let rt = runtime();
    let daemon = start_daemon(&root);

    let ids = rt.block_on(async {
        let (connected, _) = client(daemon.socket()).await;
        let api = Api::new(connected, runtime_id);
        let mut ids = Vec::new();
        let count = std::env::var("STACKLY_BUDGET_INSTANCES")
            .ok()
            .and_then(|count| count.parse().ok())
            .unwrap_or(INSTANCES);
        for index in 0..count {
            let id = api.create_ready(&format!("idle-{index}"), json!({})).await;
            api.start_ready(id).await;
            ids.push(id);
        }
        ids
    });
    // Let the start-up work settle before the first sample.
    std::thread::sleep(Duration::from_secs(20));

    // `STACKLY_BUDGET_PROFILE=<file>` also writes a `sample` of the daemon,
    // to see where its CPU goes when the number is above the budget.
    if let Some(file) = std::env::var_os("STACKLY_BUDGET_PROFILE") {
        let _ = Command::new("/usr/bin/sample")
            .args([&daemon.pid().to_string(), "20", "-file"])
            .arg(file)
            .output();
    }
    let began = Instant::now();
    let (_, cpu_before) = sample(daemon.pid());
    let mut rss_max = 0;
    let mut samples = 0_u32;
    let sample_for = std::env::var("STACKLY_BUDGET_SECONDS")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .map_or(SAMPLE_FOR, Duration::from_secs);
    while began.elapsed() < sample_for {
        let (rss, _) = sample(daemon.pid());
        rss_max = rss_max.max(rss);
        samples += 1;
        std::thread::sleep(SAMPLE_EVERY);
    }
    let (rss, cpu_after) = sample(daemon.pid());
    rss_max = rss_max.max(rss);
    let elapsed = began.elapsed().as_secs_f64();
    let cpu_percent = (cpu_after - cpu_before) / elapsed * 100.0;
    // The instances outlive the daemon, so stop them before it goes.
    rt.block_on(async {
        let (connected, _) = client(daemon.socket()).await;
        let api = Api::new(connected, runtime_id);
        for id in ids {
            let accepted = api
                .stop(id)
                .await
                .expect("the stop is accepted")
                .accepted
                .expect("a stop begins an operation");
            api.wait_final(accepted.operation.id).await;
        }
    });
    eprintln!(
        "NFR-003 supervisor: instances={INSTANCES} seconds={elapsed:.0} samples={samples} \
         rss_max_kib={rss_max} cpu_percent={cpu_percent:.3}"
    );

    assert!(rss_max <= RSS_MAX_KIB, "the daemon held {rss_max} KiB");
    assert!(
        cpu_percent <= CPU_MAX_PERCENT,
        "the daemon used {cpu_percent:.3} percent of a core"
    );
    drop(daemon);
}
