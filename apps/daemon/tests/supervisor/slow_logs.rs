//! Slow log clients (spec 0027 AC-3, AC-7, *Critical test scenarios*).
//!
//! The evidence run: the real `stacklyd` binary, a fake writing 5000 lines
//! of 512 bytes a second for 60 seconds, one follower on a raw socket that
//! never reads after the follow reply and one that reads a frame a second, while `daemon.get_info` is
//! timed every 100 ms and the physical footprint of the daemon process is
//! sampled every second. It takes more than a minute, so it runs only under
//! `cargo xtask evidence run` (which sets `STACKLY_EVIDENCE_DIR`) or with
//! `STACKLY_MEASURE=1`:
//! `cargo xtask evidence run --kind fixture -- cargo test -p stacklyd --test supervisor slow_logs -- --nocapture`.
//!
//! A client queues up to `EVENT_BUFFER_LEN` frames before it stops reading
//! the socket, about a minute of frames for a reader at one a second, so
//! within the run that reader never falls behind a rotation. The gap such a
//! reader gets is proven apart, with a follow that starts at a cursor the
//! rotation already passed.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_ipc::{ConnectPolicy, FramedConnection, connect_verified};
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::logs::{
    LogCursor, LogEntryKind, LogGapReason, LogsFollowParams, LogsReadParams,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::{CONTRACT_DIGEST, Call, LogSeq, ProtocolVersion, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{EVIDENCE_DIR_ENV, Kind, evidence};
use uuid::Uuid;

use crate::common::{client, count_messages, daemon_paths, runtime, start_daemon, uid};
use crate::support::{Api, PATIENCE, Site, fast, reap_guard, seed_root};

/// The lines the fake writes a second, stdout and stderr in turn.
const LINES_PER_SECOND: u64 = 5000;
/// The length of every line, newline included.
const LINE_BYTES: u64 = 512;
/// How long the fake writes while the daemon is measured.
const SPAM_FOR: Duration = Duration::from_secs(60);
/// How often `daemon.get_info` is timed.
const INFO_EVERY: Duration = Duration::from_millis(100);
/// How often the footprint is sampled.
const MEMORY_EVERY: Duration = Duration::from_secs(1);
/// The p99 bound of `daemon.get_info` (AC-3).
const INFO_P99_MAX: Duration = Duration::from_millis(100);
/// How far the footprint may grow over the baseline (AC-3), in KiB.
const GROWTH_MAX_KIB: u64 = 64 * 1024;
/// The message the daemon logs when it closes a connection that did not
/// read for `WRITE_TIMEOUT`.
const WRITE_TIMEOUT_MESSAGE: &str =
    "the client did not read for 30 seconds; closing the connection";
/// The share of the lines due that the fake must have written: below it,
/// the fake slowed down.
const WRITTEN_MIN_SHARE: f64 = 0.95;

/// The physical footprint of `pid` in KiB, from `footprint`, the number
/// `phys_footprint` reports.
fn footprint_kib(pid: u32) -> u64 {
    let output = Command::new("/usr/bin/footprint")
        .args(["-p", &pid.to_string()])
        .output()
        .expect("footprint runs");
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.contains("Footprint:"))
        .unwrap_or_else(|| panic!("footprint prints a total: {text}"));
    let mut words = line
        .split("Footprint:")
        .nth(1)
        .expect("the total follows the label")
        .split_whitespace();
    let value: f64 = words
        .next()
        .and_then(|value| value.parse().ok())
        .expect("the total is a number");
    let unit = words.next().expect("the total has a unit");
    let kib = match unit {
        "B" => value / 1024.0,
        "KB" => value,
        "MB" => value * 1024.0,
        "GB" => value * 1024.0 * 1024.0,
        other => panic!("an unknown footprint unit {other}"),
    };
    // A footprint is far below 2^53 KiB, so the cast loses nothing.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let kib = kib.round() as u64;
    kib
}

async fn footprint_now(pid: u32) -> u64 {
    tokio::task::spawn_blocking(move || footprint_kib(pid))
        .await
        .expect("the sample runs")
}

/// The sequence number the fake put in a spam line.
fn spam_number(text: &str) -> Option<u64> {
    text.strip_prefix("log line ")?.get(..10)?.parse().ok()
}

fn follow(id: Uuid, after: Option<LogCursor>) -> LogsFollowParams {
    LogsFollowParams {
        instance: id,
        streams: Vec::new(),
        contains: None,
        after,
    }
}

/// Sends `request` on `connection` and returns the reply frame as JSON.
async fn raw_call(
    connection: &mut FramedConnection,
    request: &serde_json::Value,
) -> serde_json::Value {
    connection
        .write_frame(&serde_json::to_vec(request).expect("a request serializes"))
        .await
        .expect("the request is sent");
    let reply = connection
        .read_frame(None)
        .await
        .expect("the reply reads")
        .expect("the daemon answers");
    serde_json::from_slice(&reply).expect("the reply is JSON")
}

/// A follower on a raw socket: it handshakes, starts a follow, reads the
/// follow reply, and never reads again.
async fn silent_follower(socket: &std::path::Path, id: Uuid) -> FramedConnection {
    let mut connection = connect_verified(socket, ConnectPolicy::same_user(uid()))
        .await
        .expect("the raw connection opens");
    let current = ProtocolVersion::CURRENT;
    let accepted = raw_call(
        &mut connection,
        &json!({
            "requestId": "silent-handshake",
            "call": { "type": "daemon.handshake", "content": {
                "protocol": { "major": current.major, "minor": current.minor },
                "client": { "kind": "cli", "version": "0.1.0" },
                "contractDigest": CONTRACT_DIGEST,
            }}
        }),
    )
    .await;
    assert_eq!(accepted["type"], "response", "the handshake: {accepted}");
    let following = raw_call(
        &mut connection,
        &json!({
            "requestId": "silent-follow",
            "call": { "type": "instance.logs_follow", "content": { "instance": id } }
        }),
    )
    .await;
    assert_eq!(following["type"], "response", "the follow: {following}");
    connection
}

fn newest(id: Uuid, limit: u32) -> Call {
    Call::InstanceLogsRead(LogsReadParams {
        instance: id,
        streams: Vec::new(),
        contains: None,
        position: None,
        limit: Some(limit),
    })
}

/// What the follower that reads a frame a second saw.
#[derive(Debug, Default)]
struct SlowReading {
    frames: usize,
    entries: usize,
    out_of_order: usize,
}

#[test]
fn a_follower_that_never_reads_slows_neither_the_daemon_nor_the_process() {
    if std::env::var_os(EVIDENCE_DIR_ENV).is_none() && std::env::var_os("STACKLY_MEASURE").is_none()
    {
        eprintln!(
            "skipped: runs under cargo xtask evidence run or with STACKLY_MEASURE=1, for more than a minute"
        );
        return;
    }
    let ev = evidence!(
        covers = ["NFR-004", "AC-017"],
        kind = Kind::Fixture,
        expected = "spec 0027 AC-3, AC-7: with the stacklyd binary, a fake writing 5000 lines of 512 bytes a second for 60 s, one follower that never reads its socket, and one that reads a frame a second, daemon.get_info sampled every 100 ms answers under 100 ms at p99, the footprint of the daemon process sampled every second grows at most 64 MiB over the baseline, the fake writes at least 95 percent of the lines due, the silent follower is closed with ipc.write_timeout, and the slow follower reads entries in seq order",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let _reap = reap_guard(&root);
    let runtime_id = seed_root(&root);
    let rt = runtime();
    let daemon = start_daemon(&root);
    let pid = daemon.pid();
    let socket = daemon.socket().to_path_buf();
    let daemon_log = daemon_paths(&root)
        .layout
        .daemon_log_dir()
        .join("stacklyd.log");

    let summary = rt.block_on(async move {
        let api = Api::new(client(&socket).await.0, runtime_id);
        let id = api
            .create_ready(
                "chatty",
                json!({ "logLinesPerSecond": LINES_PER_SECOND, "logLineBytes": LINE_BYTES }),
            )
            .await;
        // The baseline: the daemon up, the instance made, nothing written yet.
        let baseline = footprint_now(pid).await;

        api.start_ready(id).await;
        let began = Instant::now();

        let silent = silent_follower(&socket, id).await;

        let done = Arc::new(AtomicBool::new(false));
        let slow_task = {
            let slow = client(&socket).await.0;
            let done = Arc::clone(&done);
            tokio::spawn(async move {
                let (_, mut stream) = slow
                    .follow_logs(follow(id, None))
                    .await
                    .expect("the slow follow starts");
                let mut reading = SlowReading::default();
                let mut last = 0_u64;
                while !done.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let Ok(Ok(Some(DecodedFrame::Log(frame)))) =
                        tokio::time::timeout(Duration::from_secs(5), stream.next()).await
                    else {
                        continue;
                    };
                    reading.frames += 1;
                    for entry in &frame.entries {
                        reading.entries += 1;
                        if entry.seq.get() <= last {
                            reading.out_of_order += 1;
                        }
                        last = entry.seq.get();
                    }
                }
                (reading, slow)
            })
        };
        let info_task = {
            let prober = client(&socket).await.0;
            let done = Arc::clone(&done);
            tokio::spawn(async move {
                let mut times = Vec::new();
                while !done.load(Ordering::SeqCst) {
                    let asked = Instant::now();
                    let reply = prober.call(Call::DaemonGetInfo).await;
                    times.push(asked.elapsed());
                    assert!(
                        matches!(reply, Ok(Reply::DaemonGetInfo(_))),
                        "daemon.get_info answers: {reply:?}"
                    );
                    tokio::time::sleep(INFO_EVERY.saturating_sub(asked.elapsed())).await;
                }
                (times, prober)
            })
        };

        let mut peak = baseline;
        let mut memory_samples = 0_u32;
        while began.elapsed() < SPAM_FOR {
            peak = peak.max(footprint_now(pid).await);
            memory_samples += 1;
            tokio::time::sleep(MEMORY_EVERY).await;
        }
        peak = peak.max(footprint_now(pid).await);
        done.store(true, Ordering::SeqCst);
        let (mut times, _prober) = info_task.await.expect("the prober ends");
        let (reading, _slow) = slow_task.await.expect("the slow follower ends");

        // The silent follower: the daemon closed it with ipc.write_timeout.
        let silent_closed = count_messages(&daemon_log, WRITE_TIMEOUT_MESSAGE);
        drop(silent);

        let stop = api
            .stop(id)
            .await
            .expect("the stop")
            .accepted
            .expect("an operation");
        let ran_for = began.elapsed();
        api.wait_state(stop.operation.id, OperationState::Succeeded)
            .await;
        let page = match api
            .client
            .call(newest(id, 50))
            .await
            .expect("the newest page reads")
        {
            Reply::InstanceLogsRead(page) => page,
            other => panic!("not a logs_read reply: {other:?}"),
        };
        let written = page
            .entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.kind {
                LogEntryKind::Line(line) => spam_number(&line.text),
                _ => None,
            })
            .map_or(0, |number| number + 1);

        times.sort_unstable();
        let p99 = times
            .get(times.len() * 99 / 100)
            .copied()
            .unwrap_or_default();
        let max = times.last().copied().unwrap_or_default();
        let growth = peak.saturating_sub(baseline);
        #[expect(clippy::cast_precision_loss)]
        let due = ran_for.as_secs_f64() * LINES_PER_SECOND as f64;
        #[expect(clippy::cast_precision_loss)]
        let share = written as f64 / due;
        let summary = format!(
            "{} get_info samples p99={} ms max={} ms; daemon footprint baseline={baseline} KiB peak={peak} KiB growth={growth} KiB over {memory_samples} samples; fake wrote {written} of {due:.0} lines due ({:.1} percent) in {:.1} s; connections closed by ipc.write_timeout {silent_closed}; slow follower read {} frames, {} entries, {} out of order",
            times.len(),
            p99.as_millis(),
            max.as_millis(),
            share * 100.0,
            ran_for.as_secs_f64(),
            reading.frames,
            reading.entries,
            reading.out_of_order,
        );
        eprintln!("AC-3 slow client: {summary}");

        assert!(p99 < INFO_P99_MAX, "{summary}");
        assert!(growth <= GROWTH_MAX_KIB, "{summary}");
        assert!(share >= WRITTEN_MIN_SHARE, "{summary}");
        assert_eq!(silent_closed, 1, "{summary}");
        assert!(reading.frames > 0, "{summary}");
        assert_eq!(reading.out_of_order, 0, "{summary}");
        summary
    });
    drop(daemon);
    ev.pass_with(&summary);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_follower_that_rotation_passed_gets_the_gap_and_then_entries_in_order() {
    let ev = evidence!(
        covers = ["REQ-017", "AC-017"],
        kind = Kind::Fixture,
        expected = "spec 0027 AC-5, AC-7: with 32 KiB segments and three per stream, a follow after a cursor that rotation passed gets a first frame with the gap rotated, then entries whose seq only rises",
    );
    let site = Site::new();
    let mut config = fast();
    config.durable_logs.segment_max_bytes = 32 * 1024;
    config.durable_logs.segments_per_stream = 3;
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let id = api
        .create_ready("behind", json!({ "logLinesPerSecond": 2000 }))
        .await;
    api.start_ready(id).await;
    // Wait until the first records are rotated out.
    let deadline = Instant::now() + PATIENCE;
    let store = loop {
        if let Reply::InstanceLogsRead(page) =
            api.client.call(newest(id, 1)).await.expect("the log reads")
            && let Some(store) = page.store_id
            && page.head.is_some_and(|head| head.seq.get() > 10_000)
        {
            break store;
        }
        assert!(Instant::now() < deadline, "the fake writes 10000 records");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };

    let behind = LogCursor {
        store_id: store,
        seq: LogSeq::new(1).expect("a seq"),
    };
    let (_, mut stream) = api
        .client
        .follow_logs(follow(id, Some(behind)))
        .await
        .expect("the follow starts");
    let mut frames = Vec::new();
    while frames.len() < 3 {
        let frame = tokio::time::timeout(PATIENCE, stream.next())
            .await
            .expect("a frame in time")
            .expect("the stream reads")
            .expect("the stream is open");
        if let DecodedFrame::Log(frame) = frame {
            frames.push(frame);
        }
    }
    assert_eq!(
        frames[0].gap.map(|gap| gap.reason),
        Some(LogGapReason::Rotated),
        "the first frame carries the gap"
    );
    let seqs: Vec<u64> = frames
        .iter()
        .flat_map(|frame| frame.entries.iter().map(|entry| entry.seq.get()))
        .collect();
    assert!(seqs.first().is_some_and(|first| *first > 2), "{seqs:?}");
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "seq only rises"
    );
    // A follower that reads slower than the rotation can be passed again, so a
    // later frame may carry a gap, but only a rotated one, and seq still rises.
    assert!(
        frames[1..].iter().all(|frame| frame
            .gap
            .is_none_or(|gap| gap.reason == LogGapReason::Rotated)),
        "a later gap is a rotated gap"
    );

    drop(stream);
    let stop = api
        .stop(id)
        .await
        .expect("the stop")
        .accepted
        .expect("an operation");
    api.wait_state(stop.operation.id, OperationState::Succeeded)
        .await;
    drop(api);
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
    ev.pass();
}

#[test]
fn spam_numbers_are_read_from_the_line_prefix() {
    assert_eq!(spam_number("log line 0000000042 on stderr ...."), Some(42));
    assert_eq!(spam_number("process started"), None);
}
