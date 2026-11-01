//! The durable log store against real files in a test root (spec 0027,
//! milestone 1): the writer thread, rotation, recovery, dedupe after a
//! crash, the bounded queue and failing writes, pages, and the follow
//! source.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use stackly_core::logs::{
    DurableLog, DurableLogConfig, FollowSource, LogCursor, LogFilter, LogGap, LogPage, LogPosition,
    LogStoreId, ReadRequest, RecordKind, StoreFailure, StreamSet, read_page,
};
use stackly_core::supervisor::{LogBatch, LogLine, RunEvent, SinkOutcome, SpoolSource};
use stackly_core::{PreparedLayout, prepare_layout};
use stackly_domain::{InstanceId, Pid, ProcessStart};
use stackly_platform::{OutputStream, Platform};
use stackly_testkit::{FakePlatform, Fault, FsOp, TestRoot};

struct Rig {
    _root: TestRoot,
    fake: FakePlatform,
    platform: Arc<Platform>,
    layout: Arc<PreparedLayout>,
    id: InstanceId,
}

impl Rig {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root");
        let fake = FakePlatform::new(&root);
        let platform = Arc::new(fake.platform());
        let layout = Arc::new(prepare_layout(&platform).expect("the layout prepares"));
        let id = InstanceId::generate();
        let rig = Self {
            _root: root,
            fake,
            platform,
            layout,
            id,
        };
        rig.instance_dir(id);
        rig
    }

    fn instance_dir(&self, id: InstanceId) {
        self.platform
            .fs()
            .create_dir_private(&self.layout.layout().instance_log_dir(id))
            .expect("the instance log folder");
    }

    fn start(&self, config: DurableLogConfig) -> DurableLog {
        DurableLog::start(Arc::clone(&self.platform), Arc::clone(&self.layout), config)
            .expect("the writer starts")
    }

    fn read(
        &self,
        log: &DurableLog,
        filter: LogFilter,
        position: LogPosition,
        limit: usize,
    ) -> LogPage {
        let request = ReadRequest {
            filter,
            position,
            limit,
            byte_limit: 512 * 1024,
            scan_budget: 8 * 1024 * 1024,
            segments_per_stream: log.config().segments_per_stream,
        };
        read_page(self.platform.fs(), &log.dir(self.id), &request).expect("the store reads")
    }

    fn newest(&self, log: &DurableLog) -> LogPage {
        self.read(log, LogFilter::all(), LogPosition::Newest, 1000)
    }
}

fn source() -> SpoolSource {
    SpoolSource::new(
        Pid::new(4123).expect("a pid"),
        ProcessStart::new(1_790_000_000, 904_512).expect("a start"),
    )
}

/// A batch of whole lines from the spool of `source()`, each `end` the
/// given offset.
fn spooled(stream: OutputStream, lines: &[(&str, u64)]) -> LogBatch {
    let lines = lines
        .iter()
        .map(|(text, end)| {
            let mut line = LogLine::new(*text);
            line.end = Some(*end);
            line
        })
        .collect();
    LogBatch::new(stream, Timestamp::UNIX_EPOCH, lines).with_source(source())
}

fn texts(page: &LogPage) -> Vec<String> {
    page.entries
        .iter()
        .filter_map(|entry| match &entry.kind {
            RecordKind::Line(line) => Some(line.text.clone()),
            _ => None,
        })
        .collect()
}

fn idle(log: &DurableLog) {
    assert!(
        log.wait_idle(Duration::from_secs(10)),
        "the writer drained its queue"
    );
}

#[test]
fn lines_of_both_streams_are_stored_with_one_rising_seq_and_merged_on_read() {
    let rig = Rig::new();
    let log = rig.start(DurableLogConfig::default());
    log.run_event(
        rig.id,
        RunEvent::Started {
            pid: Pid::new(4123).unwrap(),
            at: Timestamp::UNIX_EPOCH,
        },
    );
    log.accept(
        rig.id,
        spooled(OutputStream::Stdout, &[("one", 4), ("two", 8)]),
    );
    log.accept(
        rig.id,
        spooled(OutputStream::Stderr, &[("FATAL: three", 13)]),
    );
    log.accept(rig.id, spooled(OutputStream::Stdout, &[("four", 13)]));
    log.run_event(
        rig.id,
        RunEvent::Exited {
            code: Some(1),
            signal: None,
            at: Timestamp::UNIX_EPOCH,
        },
    );
    idle(&log);

    let page = rig.newest(&log);

    let seqs: Vec<u64> = page.entries.iter().map(|entry| entry.seq).collect();
    assert_eq!(seqs, [1, 2, 3, 4, 5, 6]);
    assert_eq!(texts(&page), ["one", "two", "FATAL: three", "four"]);
    assert!(matches!(
        page.entries[0].kind,
        RecordKind::Started { pid: 4123 }
    ));
    assert_eq!(page.entries[0].stream, None);
    assert!(matches!(
        page.entries[5].kind,
        RecordKind::Exited { code: Some(1), .. }
    ));
    let RecordKind::Line(fatal) = &page.entries[3].kind else {
        panic!("a line")
    };
    assert_eq!(fatal.level, stackly_core::logs::LogLevel::Error);
    assert_eq!(page.entries[3].stream, Some(OutputStream::Stderr));
    assert_eq!(page.head, Some(6));
    assert!(page.store.is_some());
    assert_eq!(page.earlier, None);
    assert_eq!(page.later, None);

    let stderr = rig.read(
        &log,
        LogFilter::new(StreamSet::only(OutputStream::Stderr), None),
        LogPosition::Newest,
        100,
    );
    assert_eq!(texts(&stderr), ["FATAL: three"]);
    assert_eq!(stderr.entries.len(), 3, "the run markers are in both files");

    let found = rig.read(
        &log,
        LogFilter::new(StreamSet::ALL, Some("FOU")),
        LogPosition::Newest,
        100,
    );
    assert_eq!(texts(&found), ["four"]);
    assert_eq!(
        found.entries.len(),
        1,
        "start and exit markers hide behind a text filter"
    );

    let dir = log.dir(rig.id);
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    for entry in std::fs::read_dir(&dir).unwrap() {
        assert_eq!(mode(&entry.unwrap().path()), 0o600);
    }
    log.shutdown();
}

#[test]
fn rotation_keeps_at_most_the_segment_limit_per_stream_and_spares_the_other_stream() {
    let rig = Rig::new();
    let mut config = DurableLogConfig::default();
    config.segment_max_bytes = 2048;
    config.segments_per_stream = 3;
    let log = rig.start(config);
    log.accept(
        rig.id,
        spooled(OutputStream::Stderr, &[("kept on stderr", 15)]),
    );
    idle(&log);
    let store = rig.newest(&log).store.unwrap();
    for round in 0..60_u64 {
        let text = format!("{round:04} {}", "x".repeat(100));
        log.accept(
            rig.id,
            spooled(OutputStream::Stdout, &[(text.as_str(), (round + 1) * 106)]),
        );
        idle(&log);
    }

    let names: Vec<String> = std::fs::read_dir(log.dir(rig.id))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("stdout."))
            .count(),
        3
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("stderr."))
            .count(),
        1
    );
    let page = rig.newest(&log);
    assert!(page.stored_bytes[0] <= 3 * (2048 + 300));
    assert!(texts(&page).contains(&"kept on stderr".to_owned()));

    let old = rig.read(
        &log,
        LogFilter::new(StreamSet::only(OutputStream::Stdout), None),
        LogPosition::After(LogCursor { store, seq: 2 }),
        5,
    );
    assert_eq!(old.gap, Some(LogGap::Rotated));
    assert!(old.entries[0].seq > 3);
    log.shutdown();
}

#[test]
fn lines_seen_again_after_a_crash_are_dropped_and_lost_bytes_leave_a_truncated_marker() {
    let rig = Rig::new();
    let log = rig.start(DurableLogConfig::default());
    log.accept(
        rig.id,
        spooled(OutputStream::Stdout, &[("a", 2), ("b", 4), ("c", 6)]),
    );
    idle(&log);
    log.shutdown();

    // A new daemon reads the spool again from offset 2 (b and c were stored).
    let log = rig.start(DurableLogConfig::default());
    let mut again = spooled(OutputStream::Stdout, &[("b", 4), ("c", 6), ("d", 8)]).resumed();
    again.resumed_from = Some(2);
    log.accept(rig.id, again);
    idle(&log);
    assert_eq!(texts(&rig.newest(&log)), ["a", "b", "c", "d"]);
    log.shutdown();

    // A daemon that died with accepted lines unwritten: the spool resumes
    // at 20 while the store ends at 8.
    let log = rig.start(DurableLogConfig::default());
    let mut lost = spooled(OutputStream::Stdout, &[("k", 22)]).resumed();
    lost.resumed_from = Some(20);
    log.accept(rig.id, lost);
    idle(&log);
    let page = rig.newest(&log);
    assert!(matches!(
        page.entries[4].kind,
        RecordKind::Truncated { bytes: 12 }
    ));
    assert_eq!(texts(&page), ["a", "b", "c", "d", "k"]);
    let seqs: Vec<u64> = page.entries.iter().map(|entry| entry.seq).collect();
    assert_eq!(seqs, [1, 2, 3, 4, 5, 6]);
    assert_eq!(log.health().truncations, 1);
    assert_eq!(log.health().bytes_truncated, 12);
    log.shutdown();
}

#[test]
fn the_pieces_of_one_long_line_are_kept_whole_and_a_new_run_is_never_dropped() {
    let rig = Rig::new();
    let log = rig.start(DurableLogConfig::default());
    log.accept(
        rig.id,
        spooled(OutputStream::Stdout, &[("p1", 50), ("p2", 50), ("p3", 50)]),
    );
    idle(&log);
    let other = SpoolSource::new(
        Pid::new(9).unwrap(),
        ProcessStart::new(1_790_000_100, 0).unwrap(),
    );
    let mut next = LogBatch::new(
        OutputStream::Stdout,
        Timestamp::UNIX_EPOCH,
        vec![{
            let mut line = LogLine::new("new run");
            line.end = Some(3);
            line
        }],
    )
    .with_source(other);
    next.resumed = false;
    log.accept(rig.id, next);
    idle(&log);

    assert_eq!(texts(&rig.newest(&log)), ["p1", "p2", "p3", "new run"]);
    log.shutdown();
}

#[test]
fn recovery_trims_a_cut_line_skips_bad_records_ignores_strangers_and_survives_a_lost_header() {
    let rig = Rig::new();
    let log = rig.start(DurableLogConfig::default());
    log.accept(
        rig.id,
        spooled(OutputStream::Stdout, &[("one", 4), ("two", 8)]),
    );
    idle(&log);
    let first_store = rig.newest(&log).store.unwrap();
    log.shutdown();

    let dir = log.dir(rig.id);
    let segment = dir.join("stdout.00000000000000000001.jsonl");
    let mut bytes = std::fs::read(&segment).unwrap();
    let second = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    bytes.splice(second..second, b"not a record\n".iter().copied());
    bytes.extend_from_slice(b"{\"seq\":3,\"at\":\"2026-");
    std::fs::write(&segment, &bytes).unwrap();
    std::fs::write(dir.join("notes.txt"), b"mine").unwrap();
    std::fs::remove_file(dir.join("store.json")).unwrap();

    let log = rig.start(DurableLogConfig::default());
    log.accept(rig.id, spooled(OutputStream::Stdout, &[("three", 14)]));
    idle(&log);
    let page = rig.newest(&log);

    assert_eq!(texts(&page), ["one", "two", "three"]);
    assert_eq!(page.entries.last().unwrap().seq, 3);
    assert_eq!(page.skipped_records, 1);
    assert_ne!(
        page.store.unwrap(),
        first_store,
        "a lost header makes a new store"
    );
    assert!(dir.join("notes.txt").exists());
    log.shutdown();
}

#[test]
fn a_full_queue_answers_busy_and_a_failing_store_parks_only_its_instance() {
    let rig = Rig::new();
    let other = InstanceId::generate();
    rig.instance_dir(other);
    rig.fake.fs().fail_when(
        FsOp::CreateDirPrivate,
        format!("{}/captured", rig.id),
        Fault::PermissionDenied,
    );
    let mut config = DurableLogConfig::default();
    config.queue_max = 4096;
    let log = rig.start(config);

    assert_eq!(
        log.accept(rig.id, spooled(OutputStream::Stdout, &[("first", 6)])),
        SinkOutcome::Accepted
    );
    let big = "y".repeat(5000);
    assert_eq!(
        log.accept(
            rig.id,
            spooled(OutputStream::Stdout, &[(big.as_str(), 5007)])
        ),
        SinkOutcome::Busy
    );
    for _ in 0..100 {
        log.run_event(
            rig.id,
            RunEvent::Started {
                pid: Pid::new(1).unwrap(),
                at: Timestamp::UNIX_EPOCH,
            },
        );
        log.run_event(
            rig.id,
            RunEvent::Exited {
                code: Some(1),
                signal: None,
                at: Timestamp::UNIX_EPOCH,
            },
        );
    }
    log.accept(
        other,
        spooled(OutputStream::Stdout, &[("other is fine", 14)]),
    );
    let started = std::time::Instant::now();
    while log.health().failing == 0 && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let request = ReadRequest {
        filter: LogFilter::all(),
        position: LogPosition::Newest,
        limit: 10,
        byte_limit: 4096,
        scan_budget: 1 << 20,
        segments_per_stream: 5,
    };
    let mut other_page = LogPage::default();
    while other_page.entries.is_empty() && started.elapsed() < Duration::from_secs(5) {
        other_page = read_page(rig.platform.fs(), &log.dir(other), &request).unwrap();
    }
    assert_eq!(texts(&other_page), ["other is fine"]);
    let health = log.health();
    assert!(health.degraded);
    assert_eq!(health.reason, Some(StoreFailure::PermissionDenied));
    assert_eq!(health.failing, 1);

    rig.fake.fs().clear_faults();
    rig.fake.clock().advance(SignedDuration::from_secs(1));
    log.nudge();
    idle(&log);
    let page = rig.newest(&log);
    assert_eq!(texts(&page), ["first"]);
    let markers = page.entries.len() - 1;
    assert!(markers <= 64, "{markers} run markers");
    assert!(
        log.health().degraded,
        "degraded for a while after the last failure"
    );
    assert_eq!(log.health().failing, 0);
    rig.fake.clock().advance(SignedDuration::from_secs(61));
    assert!(!log.health().degraded);
    log.shutdown();
}

#[test]
fn pages_walk_back_and_forth_by_cursor_and_stop_at_the_scan_budget() {
    let rig = Rig::new();
    let log = rig.start(DurableLogConfig::default());
    let lines: Vec<(String, u64)> = (1..=50).map(|n| (format!("line {n}"), n * 10)).collect();
    let refs: Vec<(&str, u64)> = lines
        .iter()
        .map(|(text, end)| (text.as_str(), *end))
        .collect();
    log.accept(rig.id, spooled(OutputStream::Stdout, &refs));
    idle(&log);
    let store = rig.newest(&log).store.unwrap();

    let newest = rig.read(&log, LogFilter::all(), LogPosition::Newest, 10);
    assert_eq!(newest.entries.first().unwrap().seq, 41);
    assert_eq!(newest.earlier, Some(41));
    let before = rig.read(
        &log,
        LogFilter::all(),
        LogPosition::Before(LogCursor { store, seq: 41 }),
        10,
    );
    assert_eq!(
        before.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        (31..=40).collect::<Vec<_>>()
    );
    assert_eq!(before.later, Some(40));
    let after = rig.read(
        &log,
        LogFilter::all(),
        LogPosition::After(LogCursor { store, seq: 5 }),
        10,
    );
    assert_eq!(after.entries.first().unwrap().seq, 6);
    assert_eq!(after.later, Some(15));
    let last = rig.read(
        &log,
        LogFilter::all(),
        LogPosition::After(LogCursor { store, seq: 45 }),
        10,
    );
    assert_eq!(last.entries.len(), 5);
    assert_eq!(last.later, None);
    let first = rig.read(
        &log,
        LogFilter::all(),
        LogPosition::Before(LogCursor { store, seq: 3 }),
        10,
    );
    assert_eq!(first.entries.len(), 2);
    assert_eq!(first.earlier, None);

    let reset = rig.read(
        &log,
        LogFilter::all(),
        LogPosition::After(LogCursor {
            store: LogStoreId::generate(),
            seq: 40,
        }),
        3,
    );
    assert_eq!(reset.gap, Some(LogGap::StoreReset));
    assert_eq!(reset.entries.first().unwrap().seq, 1);

    let request = ReadRequest {
        filter: LogFilter::new(StreamSet::ALL, Some("nothing matches")),
        position: LogPosition::Newest,
        limit: 10,
        byte_limit: 4096,
        scan_budget: 1,
        segments_per_stream: 5,
    };
    let limited = read_page(rig.platform.fs(), &log.dir(rig.id), &request).unwrap();
    assert!(limited.scan_limited);
    assert!(limited.entries.is_empty());
    assert!(limited.earlier.is_some());
    log.shutdown();
}

#[test]
fn a_follower_adopts_a_new_store_and_reads_only_what_comes_after_it() {
    let rig = Rig::new();
    let log = rig.start(DurableLogConfig::default());
    let dir = log.dir(rig.id);
    let mut follow = FollowSource::new(LogFilter::all(), None);

    let step = follow
        .step(rig.platform.fs(), &dir, 100, 1 << 20, 1 << 23, 5)
        .unwrap();
    assert!(step.entries.is_empty());

    let mut heads = log.subscribe(rig.id);
    log.accept(
        rig.id,
        spooled(OutputStream::Stdout, &[("one", 4), ("two", 8)]),
    );
    idle(&log);
    assert!(heads.has_changed().unwrap());
    assert_eq!(heads.borrow_and_update().head, Some(2));
    let step = follow
        .step(rig.platform.fs(), &dir, 100, 1 << 20, 1 << 23, 5)
        .unwrap();
    assert_eq!(step.entries.len(), 2);
    assert_eq!(step.gap, None);

    log.accept(rig.id, spooled(OutputStream::Stderr, &[("three", 6)]));
    idle(&log);
    let step = follow
        .step(rig.platform.fs(), &dir, 100, 1 << 20, 1 << 23, 5)
        .unwrap();
    assert_eq!(step.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), [3]);
    let step = follow
        .step(rig.platform.fs(), &dir, 100, 1 << 20, 1 << 23, 5)
        .unwrap();
    assert!(step.entries.is_empty());

    log.remove(rig.id);
    assert!(heads.borrow_and_update().removed);
    assert_eq!(
        log.accept(rig.id, spooled(OutputStream::Stdout, &[("late", 9)])),
        SinkOutcome::Accepted
    );
    log.shutdown();
}
