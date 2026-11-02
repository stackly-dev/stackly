//! The overhead of the daemon with metrics on (spec 0028 AC-14, NFR-003): the
//! real `stacklyd` binary with ten idle instances that are running and ready,
//! readiness, health, sampling, and the scheduled data size scan all active, no
//! log spam, measured for five minutes from outside the daemon with `ps`
//! (never by the sampler of the daemon). The daemon may hold at most 150 MiB of
//! resident memory and use at most 1 percent of one logical core on average.
//!
//! Three runs, each a native test with its own evidence record:
//! 1. ten fake instances and no client;
//! 2. the same with one client that reads `instance.get_metrics` of all ten
//!    every 5 seconds, with the cursor of the last reply, as the app does;
//! 3. nine fake instances and one PostgreSQL 17 instance (a tree of several
//!    processes), `BLOCKED` when the artifact is not in the runtime cache.
//!
//! Run them with `cargo xtask evidence run --kind native -- cargo test -p
//! stacklyd --test metrics_nfr003 -- --ignored --test-threads 1`.
//! `STACKLY_BUDGET_SECONDS=<n>` shortens the measurement for a trial run;
//! a run recorded as evidence uses the five minutes.
#![cfg(target_os = "macos")]

#[path = "common/mod.rs"]
mod common;
#[path = "postgresql/harness.rs"]
mod harness;
#[path = "supervisor/metrics_calls.rs"]
mod metrics_calls;
#[path = "supervisor/support.rs"]
mod support;

use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::common::{client, start_daemon};
use crate::harness::{PG17, Site, create_instance};
use crate::metrics_calls::metrics;
use crate::support::{Api, reap_guard, seed_root};

/// The most resident memory the daemon may hold, in KiB (150 MiB).
const RSS_MAX_KIB: u64 = 150 * 1024;
/// The most CPU the daemon may use on average, in percent of one logical core.
const CPU_MAX_PERCENT: f64 = 1.0;
/// How many instances run idle.
const INSTANCES: usize = 10;
/// How long the daemon is measured.
const MEASURE_FOR: Duration = Duration::from_secs(300);
/// How often `ps` reads the daemon.
const SAMPLE_EVERY: Duration = Duration::from_secs(5);
/// How often the client of the second run reads the metrics.
const CLIENT_EVERY: Duration = Duration::from_secs(5);

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

/// What a measurement found.
struct Measured {
    seconds: f64,
    samples: u32,
    rss_max_kib: u64,
    cpu_percent: f64,
}

/// Reads the daemon `pid` every few seconds for `duration`, from outside.
async fn measure(pid: u32, duration: Duration) -> Measured {
    let began = Instant::now();
    let (_, cpu_before) = sample(pid);
    let mut rss_max = 0;
    let mut samples = 0_u32;
    while began.elapsed() < duration {
        rss_max = rss_max.max(sample(pid).0);
        samples += 1;
        tokio::time::sleep(SAMPLE_EVERY).await;
    }
    let (rss, cpu_after) = sample(pid);
    rss_max = rss_max.max(rss);
    let seconds = began.elapsed().as_secs_f64();
    Measured {
        seconds,
        samples,
        rss_max_kib: rss_max,
        cpu_percent: (cpu_after - cpu_before) / seconds * 100.0,
    }
}

fn duration() -> Duration {
    std::env::var("STACKLY_BUDGET_SECONDS")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .map_or(MEASURE_FOR, Duration::from_secs)
}

/// A client that reads `instance.get_metrics` of every instance on a fixed
/// beat, each with the cursor of its last reply, until it is aborted.
fn read_metrics_every_five_seconds(api: Api, ids: Vec<Uuid>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut cursors: HashMap<Uuid, (String, u64)> = HashMap::new();
        loop {
            for id in &ids {
                let cursor = cursors.get(id).cloned();
                let reply = metrics(
                    &api,
                    *id,
                    cursor.as_ref().map(|(history, _)| history.as_str()),
                    cursor.as_ref().map(|(_, seq)| *seq),
                )
                .await
                .expect("a metrics reply");
                if let (Some(history), Some(last)) = (&reply.history_id, reply.samples.last()) {
                    cursors.insert(*id, (history.clone(), last.seq.get()));
                } else if reply.history_id.is_none() {
                    cursors.remove(id);
                }
            }
            tokio::time::sleep(CLIENT_EVERY).await;
        }
    })
}

/// Stops each instance and waits for the stop, since instances outlive the
/// daemon.
async fn stop_all(api: &Api, ids: &[Uuid]) {
    for id in ids {
        let accepted = api
            .stop(*id)
            .await
            .expect("the stop is accepted")
            .accepted
            .expect("a stop begins an operation");
        api.wait_final(accepted.operation.id).await;
    }
}

/// Checks the two budgets and writes the numbers on the evidence record.
fn judge(label: &str, found: &Measured, evidence: &Evidence) {
    let line = format!(
        "{label}: instances={INSTANCES} seconds={:.0} samples={} rss_max_kib={} cpu_percent={:.3}",
        found.seconds, found.samples, found.rss_max_kib, found.cpu_percent
    );
    eprintln!("NFR-003 {line}");
    assert!(
        found.rss_max_kib <= RSS_MAX_KIB,
        "the daemon held {} KiB ({line})",
        found.rss_max_kib
    );
    assert!(
        found.cpu_percent <= CPU_MAX_PERCENT,
        "the daemon used {:.3} percent of a core ({line})",
        found.cpu_percent
    );
    evidence.pass_with(&line);
}

/// Ten fake instances on a fresh root, served by the `stacklyd` binary.
async fn run_fake(ev: Evidence, with_client: bool, label: &str) {
    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let runtime_id = seed_root(&root);
    let daemon = start_daemon(&root);
    let (connected, _) = client(daemon.socket()).await;
    let api = Api::new(connected, runtime_id);
    let mut ids = Vec::new();
    for index in 0..INSTANCES {
        let id = api.create_ready(&format!("idle-{index}"), json!({})).await;
        api.start_ready(id).await;
        ids.push(id);
    }
    // Let the start up work settle before the first sample.
    tokio::time::sleep(Duration::from_secs(20)).await;

    let reading = if with_client {
        let api = api_clone(&daemon, runtime_id).await;
        Some(read_metrics_every_five_seconds(api, ids.clone()))
    } else {
        None
    };
    let found = measure(daemon.pid(), duration()).await;
    if let Some(reading) = reading {
        reading.abort();
    }
    stop_all(&api, &ids).await;
    judge(label, &found, &ev);
    drop(daemon);
}

async fn api_clone(daemon: &stackly_testkit::SpawnedDaemon, runtime_id: Uuid) -> Api {
    let (connected, _) = client(daemon.socket()).await;
    Api::new(connected, runtime_id)
}

// covers: AC-14
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: measures the stacklyd binary with ten idle instances for five minutes"]
async fn ten_idle_fake_instances_stay_inside_the_budget_with_no_client() {
    let ev = evidence!(
        covers = ["NFR-003", "REQ-018"],
        kind = Kind::Native,
        expected = "with ten idle fake instances running, probes, health, sampling, and the scheduled data size scan on, and no client, the daemon holds at most 150 MiB of resident memory and uses at most 1 percent of one logical core over five minutes, measured from outside by ps",
    );
    run_fake(ev, false, "no client").await;
}

// covers: AC-14
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: measures the stacklyd binary with ten idle instances for five minutes"]
async fn ten_idle_fake_instances_stay_inside_the_budget_with_a_client_reading_metrics() {
    let ev = evidence!(
        covers = ["NFR-003", "REQ-018"],
        kind = Kind::Native,
        expected = "with ten idle fake instances and one client that reads instance.get_metrics of all ten every five seconds with its cursor, the daemon holds at most 150 MiB of resident memory and uses at most 1 percent of one logical core over five minutes, measured from outside by ps",
    );
    run_fake(ev, true, "client every 5 s").await;
}

// covers: AC-14, AC-4
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact and measures for five minutes"]
async fn nine_fake_instances_and_one_postgresql_17_stay_inside_the_budget() {
    let ev = evidence!(
        covers = ["NFR-003", "REQ-018"],
        kind = Kind::Native,
        expected = "with nine idle fake instances and one PostgreSQL 17 instance (a tree of several processes) running, the daemon holds at most 150 MiB of resident memory and uses at most 1 percent of one logical core over five minutes, measured from outside by ps, and the metrics of the PostgreSQL instance report more than one process",
    );
    let Some(site) = Site::open(&ev, PG17) else {
        return;
    };
    let fake_runtime = seed_root(&site.root);
    let daemon = site.binary(&[]);
    let api_pg = site.binary_api(&daemon).await;
    let fake = {
        let (connected, _) = client(&daemon.socket).await;
        Api::new(connected, fake_runtime)
    };
    let mut ids = Vec::new();
    for index in 0..INSTANCES - 1 {
        let id = fake.create_ready(&format!("idle-{index}"), json!({})).await;
        fake.start_ready(id).await;
        ids.push(id);
    }
    let made = create_instance(&site, &api_pg, "pg").await;
    api_pg.start_ready(made.id).await;
    let version = site
        .psql(made.id, made.port)
        .scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    tokio::time::sleep(Duration::from_secs(20)).await;

    let found = measure(daemon.pid(), duration()).await;

    let reply = metrics(&api_pg, made.id, None, None)
        .await
        .expect("a metrics reply");
    let tree = reply
        .samples
        .last()
        .expect("a sample")
        .tree
        .process_count
        .get();
    stop_all(&fake, &ids).await;
    stop_all(&api_pg, &[made.id]).await;
    assert!(tree > 1, "the PostgreSQL tree holds {tree} process");
    judge("nine fake and one PostgreSQL 17", &found, &ev);
    daemon.stop();
}
