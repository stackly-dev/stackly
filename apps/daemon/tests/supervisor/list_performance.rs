//! `instance.list` with 100 instances (spec 0022 AC-14, NFR-002): the first
//! snapshot of the app starts with this reply, so it has to come back well
//! inside the 2 second budget of the whole snapshot.

use std::time::{Duration, Instant};

use serde_json::json;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::instance::ListInstancesParams;
use stackly_testkit::evidence::{Kind, evidence};

use crate::support::Site;

const INSTANCES: usize = 100;

/// The bound a timing is checked against: the spec's own in an evidence run
/// (`cargo xtask evidence run`, which records the numbers), and ten times
/// looser in a plain `cargo test`, where other work on this Mac stretches
/// every read. The loose bound still catches a reply that walks the registry
/// once per instance.
fn bound(spec: Duration) -> Duration {
    if std::env::var_os(stackly_testkit::evidence::EVIDENCE_DIR_ENV).is_some() {
        spec
    } else {
        spec * 10
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_list_of_100_instances_is_answered_in_at_most_500_ms() {
    let ev = evidence!(
        covers = ["NFR-002"],
        kind = Kind::Fixture,
        expected = "instance.list with limit 200 answers 100 fake instances, made by instance.create with automatic ports and never started, in at most 500 ms",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    // The instances are made outside the timed call.
    for number in 0..INSTANCES {
        api.create_ready(&format!("perf-{number:03}"), json!({}))
            .await;
    }

    let started = Instant::now();
    let page = api
        .list(ListInstancesParams {
            limit: Some(200),
            before: None,
        })
        .await;
    let elapsed = started.elapsed();

    assert_eq!(page.items.len(), INSTANCES, "every instance is on the page");
    assert!(page.next.is_none(), "limit 200 holds all 100 in one page");
    let summary = format!(
        "instance.list of {INSTANCES} instances answered in {} ms",
        elapsed.as_millis()
    );
    assert!(elapsed <= bound(Duration::from_millis(500)), "{summary}");
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass_with(&summary);
}
