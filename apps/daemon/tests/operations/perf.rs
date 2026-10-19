//! Timing on this Mac: the begin latency over a real `SqliteRegistry` and
//! what the socket adds (NFR-006), and a held step that never holds the
//! registry (AC-12).

use std::time::{Duration, Instant};

use serde_json::json;
use stackly_core::operation::testing::{HoldHandle, ScriptedWork};
use stackly_core::operation::{BeginOutcome, Origin, RequestOrigin};
use stackly_domain::{OperationKind, Registry, WriteBatch, WriteOp};
use stackly_protocol::Call;
use stackly_protocol::Reply;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::operation::OperationState;
use stackly_testkit::RuntimeBuilder;
use stackly_testkit::evidence::{Kind, evidence};
use stacklyd::ipc::operation::TestStartingMethod;

use crate::support::{Raw, Script, Scripts, Site, accepted, client, key, wait_state};

const BEGINS: usize = 100;

fn percentile(times: &mut [Duration], at: usize) -> Duration {
    times.sort();
    times[(times.len() * at / 100).min(times.len() - 1)]
}

/// Waits until no operation runs or waits.
async fn drained(client: &stackly_ipc::Client) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let Ok(Reply::OperationGetStats(stats)) = client.call(Call::OperationGetStats).await else {
            panic!("operation.get_stats answers with stats");
        };
        if stats.running == 0 && stats.queued == 0 {
            return;
        }
        assert!(Instant::now() < deadline, "operations still run: {stats:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hundred_begins_have_a_p99_of_at_most_500_ms_and_the_socket_adds_under_50_ms() {
    let ev = evidence!(
        covers = ["NFR-006"],
        kind = Kind::Fixture,
        expected = "100 sequential begins on a SqliteRegistry in a test root have a p99 of at most 500 ms, and starting the same operation over the daemon socket through start_operation adds less than 50 ms at p50 and p99",
    );
    let site = Site::exclusive();
    let scripts = Scripts::new();
    scripts.add(
        "quick",
        Script::new(
            OperationKind::InstallRuntime,
            ScriptedWork::new().step("apply"),
        )
        .creating(),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let engine = daemon
        .core()
        .operations()
        .engine()
        .expect("the engine runs");

    let mut direct = Vec::with_capacity(BEGINS);
    for _ in 0..BEGINS {
        let prepared = scripts
            .prepare(Some(&json!({ "script": "quick" })))
            .unwrap();
        let origin = Origin::Request(RequestOrigin {
            method: "test.start_operation".parse().unwrap(),
            key: key().parse().unwrap(),
            fingerprint: stackly_domain::Sha256Digest::from_bytes(prepared.fingerprint),
            client: "cli".parse().unwrap(),
            interactive: false,
        });
        let request = (prepared.build)(origin);
        let started = Instant::now();
        let outcome = engine.begin(request).await.unwrap();
        direct.push(started.elapsed());
        assert!(matches!(outcome, BeginOutcome::Created(_)));
    }
    drained(&client).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let mut socket = Vec::with_capacity(BEGINS);
    for _ in 0..BEGINS {
        let started = Instant::now();
        let frame = raw.start(Some(&key()), json!({ "script": "quick" })).await;
        socket.push(started.elapsed());
        assert!(!accepted(&frame).replayed);
    }
    drained(&client).await;

    let (direct_p50, direct_p99) = (percentile(&mut direct, 50), percentile(&mut direct, 99));
    let (socket_p50, socket_p99) = (percentile(&mut socket, 50), percentile(&mut socket, 99));
    let added_p50 = socket_p50.saturating_sub(direct_p50);
    let added_p99 = socket_p99.saturating_sub(direct_p99);
    let summary = format!(
        "begin p50 {} us, p99 {} us; over the socket p50 {} us, p99 {} us; added p50 {} us, p99 {} us",
        direct_p50.as_micros(),
        direct_p99.as_micros(),
        socket_p50.as_micros(),
        socket_p99.as_micros(),
        added_p50.as_micros(),
        added_p99.as_micros(),
    );
    assert!(direct_p99 <= Duration::from_millis(500), "{summary}");
    assert!(added_p50 < Duration::from_millis(50), "{summary}");
    assert!(added_p99 < Duration::from_millis(50), "{summary}");
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass_with(&summary);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn while_a_step_is_held_for_5_s_registry_commits_and_reads_each_finish_under_50_ms() {
    let ev = evidence!(
        covers = ["REQ-048"],
        kind = Kind::Fixture,
        expected = "while an operation step is held for 5 seconds, 50 commits and 50 reads on the daemon's SqliteRegistry each finish in under 50 ms, so no transaction stays open across the step",
    );
    let site = Site::exclusive();
    let scripts = Scripts::new();
    let hold = HoldHandle::new();
    scripts.add(
        "held",
        Script::new(
            OperationKind::InstallRuntime,
            ScriptedWork::new().step("download").hold(&hold),
        ),
    );
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let id = accepted(&raw.start(Some(&key()), json!({ "script": "held" })).await)
        .operation
        .id;
    tokio::time::timeout(Duration::from_secs(10), hold.started())
        .await
        .unwrap();
    let held_at = Instant::now();
    let release = {
        let hold = hold.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            hold.release();
        })
    };
    let registry = daemon.core().registry().expect("the registry is ready");
    let operation = stackly_domain::OperationId::from_uuid(id).unwrap();

    let mut commits = Vec::new();
    let mut reads = Vec::new();
    for _ in 0..50 {
        let runtime = RuntimeBuilder::new();
        let batch = WriteBatch::new(vec![
            WriteOp::InsertArtifact(runtime.artifact()),
            WriteOp::InsertRuntime(runtime.build()),
        ]);
        let writer = registry.clone();
        let started = Instant::now();
        tokio::task::spawn_blocking(move || writer.commit(batch))
            .await
            .unwrap()
            .expect("the commit goes through");
        commits.push(started.elapsed());

        let reader = registry.clone();
        let started = Instant::now();
        let read = tokio::task::spawn_blocking(move || reader.operation(operation))
            .await
            .unwrap()
            .expect("the read goes through");
        reads.push(started.elapsed());
        assert_eq!(
            read.value.map(|op| op.state()),
            Some(stackly_domain::OperationState::Running)
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    let window = held_at.elapsed();
    release.await.unwrap();
    wait_state(&client, id, OperationState::Succeeded).await;

    let slowest_commit = commits.iter().max().copied().unwrap_or_default();
    let slowest_read = reads.iter().max().copied().unwrap_or_default();
    let summary = format!(
        "50 commits and 50 reads in {} ms of a 5 s held step: slowest commit {} us, slowest read {} us",
        window.as_millis(),
        slowest_commit.as_micros(),
        slowest_read.as_micros(),
    );
    assert!(
        window < Duration::from_secs(5),
        "the calls ran past the hold: {summary}"
    );
    assert!(slowest_commit < Duration::from_millis(50), "{summary}");
    assert!(slowest_read < Duration::from_millis(50), "{summary}");
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass_with(&summary);
}
