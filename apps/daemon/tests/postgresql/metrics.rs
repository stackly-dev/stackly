//! The process tree of a PostgreSQL 17 instance in `instance.get_metrics`
//! (spec 0028, milestone 5; AC-2, AC-4, AC-5): the real postmaster, its
//! background processes, and a backend a session opens. The test is native,
//! `#[ignore = "native: ..."]`, and ends as `BLOCKED` when the artifact is not
//! in the runtime cache.

use std::time::Duration;

use stackly_core::metrics::SamplerConfig;
use stackly_protocol::instance::{CountMetric, MetricsReply, MetricsState, NumberMetric};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG17, Site, create_instance, stop_and_shutdown};
use crate::metrics_calls::wait_metrics;

const LIMIT: Duration = Duration::from_secs(60);

fn memory(metric: &CountMetric) -> Option<u64> {
    match metric {
        CountMetric::Value(bytes) => Some(bytes.get()),
        _ => None,
    }
}

fn cpu(metric: &NumberMetric) -> Option<f64> {
    match metric {
        NumberMetric::Value(percent) => Some(*percent),
        _ => None,
    }
}

// covers: AC-2, AC-4, AC-5
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_reports_its_whole_process_tree_with_cover_labels() {
    let ev = evidence!(
        covers = ["REQ-018", "AC-018"],
        kind = Kind::Native,
        expected = "a PostgreSQL 17 instance reports a tree of more than one process whose memory is at least the memory of the main process, a main process of one with its own count, a process list in tree order whose names are the executable file name, CPU that follows a query, and a backend that a session opens as one more process of the tree",
    );
    let Some(site) = Site::open(&ev, PG17) else {
        return;
    };
    let daemon = site
        .in_process(|config| {
            config.metrics = SamplerConfig::default().with_interval(Duration::from_millis(500));
        })
        .await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "metrics").await;
    let running = api.start_ready(made.id).await;
    let pid = running.process.as_ref().expect("a running process").pid;
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    // The postmaster starts its background processes at once; a second
    // sample gives CPU a difference to take.
    let idle: MetricsReply = wait_metrics(
        &api,
        made.id,
        "a tree of several processes",
        LIMIT,
        |reply| {
            reply.samples.len() >= 3
                && reply
                    .samples
                    .last()
                    .is_some_and(|sample| sample.tree.process_count.get() > 1)
        },
    )
    .await;
    assert_eq!(idle.state, MetricsState::Live);
    assert_eq!(idle.process.as_ref().map(|process| process.pid), Some(pid));
    let sample = idle.samples.last().expect("a sample");
    let baseline = sample.tree.process_count.get();
    assert_eq!(sample.main.process_count.get(), 1);
    assert_eq!(sample.main.measured_count.get(), 1);
    assert!(baseline > 1, "{sample:#?}");
    assert!(
        (1..=baseline).contains(&sample.tree.measured_count.get()),
        "{sample:#?}"
    );
    let main_memory = memory(&sample.main.memory).expect("the main process footprint");
    let tree_memory = memory(&sample.tree.memory).expect("the tree footprint");
    assert!(main_memory > 0);
    assert!(
        tree_memory >= main_memory,
        "tree {tree_memory} is below main {main_memory}"
    );

    // The list is the tree: the postmaster first, children below it, and the
    // name is the executable file name, never a command line.
    let rows = &idle.latest_processes;
    assert_eq!(rows.len() as u64, baseline.min(200));
    assert!(
        rows[0].is_main && rows[0].pid == pid && rows[0].depth == 0 && rows[0].parent_pid.is_none()
    );
    for (index, row) in rows.iter().enumerate().skip(1) {
        assert!(!row.is_main && row.depth >= 1, "{row:?}");
        assert!(
            rows[..index]
                .iter()
                .any(|parent| Some(parent.pid) == row.parent_pid),
            "{row:?} comes before its parent"
        );
    }
    assert!(
        rows.iter().all(|row| row.name == "postgres"),
        "{:?}",
        rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>()
    );

    // A session is one more process in the tree, and a query is CPU.
    let session = {
        let psql = psql.clone();
        let password = made.password.clone();
        std::thread::spawn(move || {
            psql.admin(
                &password,
                "SELECT count(*) FROM generate_series(1, 40000000); SELECT pg_sleep(4)",
            )
        })
    };
    let busy = wait_metrics(&api, made.id, "a backend in the tree", LIMIT, |reply| {
        reply
            .samples
            .last()
            .is_some_and(|sample| sample.tree.process_count.get() > baseline)
    })
    .await;
    let peak = busy
        .samples
        .iter()
        .filter_map(|sample| cpu(&sample.tree.cpu))
        .fold(0.0_f64, f64::max);
    assert!(
        peak > 0.0,
        "a query used no CPU in {} samples",
        busy.samples.len()
    );
    for sample in &busy.samples {
        if let (Some(tree), Some(main)) = (cpu(&sample.tree.cpu), cpu(&sample.main.cpu)) {
            assert!(
                tree + 0.05 >= main,
                "tree {tree} below main {main} in {sample:?}"
            );
        }
    }
    assert!(session.join().expect("the session ended").ok);

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "postmaster {pid} with {baseline} processes, a session made it more, tree CPU peaked at {peak:.1} percent"
    ));
}
