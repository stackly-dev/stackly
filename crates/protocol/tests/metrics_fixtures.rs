//! Checks the `instance.get_metrics` DTOs of spec 0028 against the fixtures
//! in `protocols/fixtures/ipc/` (AC-11): what each reply carries, that an
//! unknown value reads as unrecognized, and that the largest reply fits one
//! control frame.

use std::path::PathBuf;

use stackly_protocol::instance::{
    CountMetric, DataUsageMetric, InstanceLifecycle, MetricsReply, MetricsState, NumberMetric,
    UnavailableReason, UsageScope,
};
use stackly_protocol::json::{Validate, decode_request};
use stackly_protocol::limits::CONTROL_FRAME_MAX;
use stackly_protocol::{Call, Reply, ServerFrame};

fn read(file: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/ipc")
        .join(file);
    std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn metrics(file: &str) -> MetricsReply {
    let frame: ServerFrame = serde_json::from_slice(&read(file)).expect(file);
    let ServerFrame::Response(response) = frame else {
        panic!("{file}: expected a response");
    };
    let Reply::InstanceGetMetrics(reply) = response.reply else {
        panic!("{file}: expected get_metrics");
    };
    *reply
}

// covers: AC-11
#[test]
fn the_live_reply_carries_samples_with_unavailable_values_and_a_process_tree() {
    let reply = metrics("frame-response-instance-get-metrics-live.json");

    assert_eq!(reply.state, MetricsState::Live);
    assert_eq!(reply.lifecycle, InstanceLifecycle::Running);
    assert_eq!(reply.samples.len(), 3);
    assert_eq!(reply.samples[0].seq.get(), 1);
    assert!(matches!(
        reply.samples[0].main.cpu,
        NumberMetric::Unavailable(content) if content.reason == UnavailableReason::WarmingUp
    ));
    assert_eq!(reply.samples[1].main.cpu, NumberMetric::Value(1.5));
    assert_eq!(reply.samples[1].tree.scope, UsageScope::Tree);
    assert!(reply.samples[1].tree.measured_count.get() < reply.samples[1].tree.process_count.get());
    assert!(reply.samples[2].tree.cpu != NumberMetric::Value(0.0));
    assert_eq!(reply.latest_processes[0].parent_pid, None);
    assert!(reply.latest_processes[0].is_main);
    assert_eq!(reply.restart_total.get(), 2);
    assert_eq!(reply.readiness.latency_ms, Some(12));
    assert!(matches!(
        reply.data_usage,
        DataUsageMetric::Scanned(scanned) if !scanned.complete
    ));
}

// covers: AC-11
#[test]
fn the_not_running_reply_has_no_history_and_resets_the_client() {
    let reply = metrics("frame-response-instance-get-metrics-not-running.json");

    assert_eq!(reply.state, MetricsState::NotRunning);
    assert_eq!(reply.lifecycle, InstanceLifecycle::Stopped);
    assert!(reply.history_id.is_none() && reply.process.is_none());
    assert!(reply.samples.is_empty() && reply.latest_processes.is_empty());
    assert!(reply.history_reset);
}

// covers: AC-11
#[test]
fn the_reset_reply_marks_samples_the_client_must_replace() {
    let reset = metrics("frame-response-instance-get-metrics-reset.json");
    let quiet = metrics("frame-response-instance-get-metrics-no-new-samples.json");

    assert!(reset.history_reset);
    assert_eq!(
        reset.data_usage,
        DataUsageMetric::Unavailable(stackly_protocol::instance::UnavailableContent {
            reason: UnavailableReason::NotScanned
        })
    );
    assert!(!quiet.history_reset && quiet.samples.is_empty());
}

// covers: AC-3
#[test]
fn every_open_value_of_the_metrics_reads_as_unrecognized() {
    let reply = metrics("frame-response-instance-get-metrics-unknown-values.json");

    assert_eq!(reply.state, MetricsState::Unrecognized);
    assert_eq!(reply.samples[0].main.scope, UsageScope::Unrecognized);
    assert_eq!(reply.samples[0].main.cpu, NumberMetric::Unrecognized);
    assert!(matches!(
        reply.samples[0].main.memory,
        CountMetric::Unavailable(content) if content.reason == UnavailableReason::Unrecognized
    ));
    assert_eq!(reply.samples[0].main.disk_read, CountMetric::Unrecognized);
    assert_eq!(reply.data_usage, DataUsageMetric::Unrecognized);
    assert_eq!(reply.latest_processes[0].cpu, NumberMetric::Unrecognized);
}

// covers: AC-11
#[test]
fn the_request_fixtures_decode_and_the_bad_cursors_are_rejected() {
    for (file, history_id, after_seq) in [
        (
            "request-instance-get-metrics.json",
            Some("4242-1760950802.000000"),
            Some(2),
        ),
        ("request-instance-get-metrics-first.json", None, None),
    ] {
        let request = decode_request(&read(file)).expect(file);
        let Call::InstanceGetMetrics(params) = request.call else {
            panic!("{file}: expected get_metrics");
        };
        assert_eq!(params.history_id.as_deref(), history_id);
        assert_eq!(params.after_seq.map(|seq| seq.get()), after_seq);
        assert!(params.validate().is_empty());
    }
    for (file, path, code) in [
        (
            "reject-instance-get-metrics-after-seq-without-history-id.json",
            "afterSeq",
            "required",
        ),
        (
            "reject-instance-get-metrics-empty-history-id.json",
            "historyId",
            "too_short",
        ),
    ] {
        let rejection = decode_request(&read(file)).expect_err(file);
        assert_eq!(rejection.error.code(), "request.invalid_params");
        assert!(
            rejection
                .error
                .fields()
                .iter()
                .any(|field| field.path() == path && field.code() == code),
            "{file}"
        );
    }
}

// covers: AC-11
#[test]
fn the_largest_reply_fits_one_control_frame() {
    let mut reply = metrics("frame-response-instance-get-metrics-live.json");
    let sample = reply.samples[1];
    reply.samples = (1..=60)
        .map(|seq| {
            let mut next = sample;
            next.seq = stackly_protocol::Sequence::new(seq).expect("a small sequence");
            next
        })
        .collect();
    let row = reply.latest_processes[3].clone();
    reply.latest_processes = (0..200)
        .map(|n| {
            let mut next = row.clone();
            next.pid = 40_000 + n;
            next.name = "x".repeat(255);
            next
        })
        .collect();
    reply.processes_truncated = true;
    let frame = ServerFrame::Response(stackly_protocol::Response {
        request_id: "0192f3a1-0000-7000-8000-000000000a01".to_owned(),
        reply: Reply::InstanceGetMetrics(Box::new(reply)),
    });

    let bytes = serde_json::to_vec(&frame).expect("encodes");

    assert!(
        bytes.len() < usize::try_from(CONTROL_FRAME_MAX).expect("fits usize"),
        "{} bytes for 60 samples and 200 rows",
        bytes.len()
    );
}
