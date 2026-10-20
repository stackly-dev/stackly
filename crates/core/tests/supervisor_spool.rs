//! The spool reader, `SinkSet`, and `TailSink` against real spool files in a
//! test root, with the fake process port's spool reader (spec 0010, step
//! 10, AC-12). The block sampling of the real punch hole needs the macOS
//! adapter and runs with the daemon tests; here the fake reader records how
//! far the spool was freed with the same rounding rule.

use std::io::Write;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use jiff::SignedDuration;
use stackly_core::secret::Redactor;
use stackly_core::supervisor::{
    DrainContext, DrainOutcome, LogBatch, LogLine, LogSink, SinkOutcome, SinkSet, SpoolDrain,
    SupervisorConfig, TailSink,
};
use stackly_domain::InstanceId;
use stackly_platform::{
    Clock, OutputStream, ProcessError, ProcessSupervision, SecretValue, SpoolWriter,
};
use stackly_testkit::{FakeClock, FakeProcessSupervision, ProcessOp, TestRoot};

/// A sink that records every batch and can be told to be busy.
#[derive(Default)]
struct Recorder {
    batches: Mutex<Vec<LogBatch>>,
    busy: AtomicBool,
}

impl Recorder {
    fn batches(&self) -> Vec<LogBatch> {
        self.batches
            .lock()
            .expect("the recorder is not poisoned")
            .clone()
    }

    fn lines(&self) -> Vec<LogLine> {
        self.batches()
            .into_iter()
            .flat_map(|batch| batch.lines)
            .collect()
    }

    fn texts(&self) -> Vec<String> {
        self.lines().into_iter().map(|line| line.text).collect()
    }
}

impl LogSink for Recorder {
    fn accept(&self, _instance: InstanceId, batch: LogBatch) -> SinkOutcome {
        if self.busy.load(Ordering::SeqCst) {
            return SinkOutcome::Busy;
        }
        self.batches
            .lock()
            .expect("the recorder is not poisoned")
            .push(batch);
        SinkOutcome::Accepted
    }
}

struct Rig {
    _root: TestRoot,
    fake: FakeProcessSupervision,
    clock: FakeClock,
    redactor: Redactor,
    sink: Recorder,
    instance: InstanceId,
    writer: SpoolWriter,
    path: PathBuf,
}

fn config(change: impl FnOnce(&mut SupervisorConfig)) -> SupervisorConfig {
    let mut config = SupervisorConfig::default();
    change(&mut config);
    config
}

impl Rig {
    fn new(unit: u64) -> Self {
        let root = TestRoot::new().expect("a test root");
        let fake = FakeProcessSupervision::new(&root);
        fake.set_punch_unit(NonZeroU64::new(unit).expect("a unit that is not zero"));
        let path = root.run_dir().join("stdout.spool");
        let writer = fake.create_spool(&path).expect("the spool can be created");
        Self {
            _root: root,
            fake,
            clock: FakeClock::new(),
            redactor: Redactor::new(),
            sink: Recorder::default(),
            instance: InstanceId::generate(),
            writer,
            path,
        }
    }

    fn write(&self, bytes: &[u8]) {
        let mut file = self.writer.file();
        file.write_all(bytes).expect("the spool can be appended to");
    }

    fn ctx(&self) -> DrainContext<'_> {
        DrainContext::new(self.instance, &self.redactor, &self.sink, &self.clock)
    }

    fn reader(&self) -> Box<dyn stackly_platform::SpoolReader> {
        self.fake
            .open_spool(&self.path)
            .expect("the spool opens")
            .expect("the spool exists")
    }

    fn start(&self, config: &SupervisorConfig) -> SpoolDrain {
        SpoolDrain::start(OutputStream::Stdout, self.reader(), config)
    }

    fn resume(&self, config: &SupervisorConfig) -> SpoolDrain {
        SpoolDrain::resume(OutputStream::Stdout, self.reader(), config)
            .expect("the spool can be resumed")
    }

    /// Pumps until the reader has read everything or the sink is busy.
    fn pump_all(&self, drain: &mut SpoolDrain) -> DrainOutcome {
        loop {
            match drain.pump(&self.ctx()).expect("the spool can be read") {
                DrainOutcome::More => {}
                other => return other,
            }
        }
    }

    fn freed(&self) -> Option<u64> {
        self.reader().first_data_offset().expect("the offset reads")
    }
}

#[test]
fn whole_lines_lose_their_line_endings_and_bad_utf8_becomes_a_replacement_character() {
    let rig = Rig::new(16);
    rig.write(b"first\r\nbad \xff\xfe byte\nlast\n");
    let mut drain = rig.start(&SupervisorConfig::default());

    let outcome = rig.pump_all(&mut drain);

    assert_eq!(outcome, DrainOutcome::CaughtUp);
    assert_eq!(
        rig.sink.texts(),
        ["first", "bad \u{FFFD}\u{FFFD} byte", "last"]
    );
    assert!(
        rig.sink
            .lines()
            .iter()
            .all(|line| !line.partial && !line.continued)
    );
}

#[test]
fn a_line_split_across_reads_is_held_until_its_newline_arrives() {
    let rig = Rig::new(16);
    let mut drain = rig.start(&config(|c| c.spool_read_chunk = 5));
    rig.write(b"hello wor");
    rig.pump_all(&mut drain);
    assert!(rig.sink.texts().is_empty());
    assert_eq!(drain.accepted_end(), 0);

    rig.write(b"ld\nsecond\n");
    rig.pump_all(&mut drain);

    assert_eq!(rig.sink.texts(), ["hello world", "second"]);
    assert_eq!(drain.accepted_end(), 19);
}

#[test]
fn the_batch_is_stamped_with_the_daemon_clock() {
    let rig = Rig::new(16);
    rig.clock.advance(SignedDuration::from_secs(90));
    rig.write(b"a\n");
    let mut drain = rig.start(&SupervisorConfig::default());

    rig.pump_all(&mut drain);

    let batches = rig.sink.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].read_at, rig.clock.now());
    assert_eq!(batches[0].stream, OutputStream::Stdout);
    assert!(!batches[0].resumed);
    assert_eq!(batches[0].truncated, None);
}

#[test]
fn a_secret_across_the_cut_of_a_long_line_is_redacted_before_the_cut() {
    let rig = Rig::new(16);
    let secret = "hunter2-super-secret-value-1234";
    rig.redactor
        .learn(&SecretValue::new(secret.to_owned()).expect("a valid secret"));
    let mut line = "a".repeat(16 * 1024 - 8);
    line.push_str(secret);
    line.push_str(&"b".repeat(40 * 1024 - line.len()));
    rig.write(format!("{line}\nnext\n").as_bytes());
    let mut drain = rig.start(&SupervisorConfig::default());

    rig.pump_all(&mut drain);

    let lines = rig.sink.lines();
    assert!(lines.len() >= 4, "a 40 KiB line is cut into pieces");
    for piece in &lines {
        assert!(
            !piece.text.contains("hunter2"),
            "no piece holds a secret part"
        );
        assert!(!piece.text.contains("1234"));
        assert!(piece.text.len() <= 16 * 1024);
    }
    let joined: String = lines.iter().map(|piece| piece.text.as_str()).collect();
    assert!(joined.contains("****"));
    assert!(!lines[0].continued);
    assert!(
        lines[1..lines.len() - 1]
            .iter()
            .all(|piece| piece.continued)
    );
    assert_eq!(lines.last().map(|piece| piece.text.as_str()), Some("next"));
}

#[test]
fn a_line_over_the_hard_limit_goes_out_as_partial_pieces_and_its_rest_continues() {
    let rig = Rig::new(16);
    let mut drain = rig.start(&config(|c| {
        c.log_line_hard_max = 100;
        c.log_line_max = 40;
    }));
    rig.write(format!("{}\nend\n", "x".repeat(250)).as_bytes());

    rig.pump_all(&mut drain);

    let lines = rig.sink.lines();
    let joined: String = lines
        .iter()
        .take(lines.len() - 1)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(joined, "x".repeat(250));
    assert!(!lines[0].continued);
    assert!(lines[1..lines.len() - 1].iter().all(|line| line.continued));
    let partial: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.partial)
        .map(|(at, _)| at)
        .collect();
    // 100 + 100 + 50 bytes: pieces of 40, 40, 20 | 40, 40, 20 | 40, 10.
    assert_eq!(partial, [2, 5]);
    let end = lines.last().expect("the end line");
    assert_eq!(
        (end.text.as_str(), end.continued, end.partial),
        ("end", false, false)
    );
    assert_eq!(drain.accepted_end(), 255);
}

#[test]
fn a_busy_sink_holds_the_batch_and_frees_nothing_until_it_accepts() {
    let rig = Rig::new(16);
    rig.sink.busy.store(true, Ordering::SeqCst);
    let mut drain = rig.start(&SupervisorConfig::default());
    rig.write(&b"0123456789abcdefghijklmnopqrstuvwxyz\n".repeat(2));

    assert_eq!(rig.pump_all(&mut drain), DrainOutcome::Busy);
    assert_eq!(rig.pump_all(&mut drain), DrainOutcome::Busy);
    assert!(rig.sink.batches().is_empty());
    assert_eq!(drain.accepted_end(), 0);
    assert_eq!(rig.freed(), Some(0));

    rig.sink.busy.store(false, Ordering::SeqCst);
    assert_eq!(rig.pump_all(&mut drain), DrainOutcome::CaughtUp);

    assert_eq!(
        rig.sink.batches().len(),
        1,
        "the waiting batch goes out once"
    );
    assert_eq!(rig.sink.texts().len(), 2);
    assert_eq!(drain.accepted_end(), 74);
    assert_eq!(rig.freed(), Some(64));
}

#[test]
fn the_spool_is_freed_up_to_the_last_accepted_line_rounded_to_whole_units() {
    let rig = Rig::new(16);
    let mut drain = rig.start(&SupervisorConfig::default());
    rig.write(b"0123456789\n0123456789\n0123456789\npartial");

    rig.pump_all(&mut drain);

    // 3 lines end at 33; the held `partial` is never freed.
    assert_eq!(drain.accepted_end(), 33);
    assert_eq!(rig.freed(), Some(32));
}

#[test]
fn a_run_of_two_thousand_lines_a_second_ends_with_the_freed_offset_at_the_last_line() {
    let rig = Rig::new(16 * 1024);
    let mut drain = rig.start(&SupervisorConfig::default());
    let mut total = 0_u64;
    let mut counter = 0_u32;
    for _tick in 0..121 {
        let mut chunk = String::new();
        for _ in 0..1000 {
            counter += 1;
            chunk.push_str(&format!(
                "2026-01-01T00:00:00Z INFO request {counter:07} done in 12ms\n"
            ));
        }
        total += u64::try_from(chunk.len()).expect("a small chunk");
        rig.write(chunk.as_bytes());
        assert_eq!(rig.pump_all(&mut drain), DrainOutcome::CaughtUp);
        assert_eq!(drain.accepted_end(), total);
        let unit = 16 * 1024;
        assert_eq!(
            rig.freed(),
            Some(total / unit * unit).filter(|&freed| freed < total)
        );
    }

    assert!(total > 6_000_000);
    assert_eq!(rig.sink.lines().len(), 121_000);
}

#[test]
fn unread_bytes_over_the_backlog_limit_are_dropped_from_the_oldest_with_a_marker() {
    let rig = Rig::new(16);
    let mut drain = rig.start(&config(|c| c.spool_backlog_max = 200));
    let mut data = String::new();
    for n in 0..100 {
        data.push_str(&format!("line-{n:03}\n"));
    }
    rig.write(data.as_bytes());

    rig.pump_all(&mut drain);

    let batches = rig.sink.batches();
    assert_eq!(batches.len(), 1);
    let truncated = batches[0]
        .truncated
        .expect("the batch carries a truncation marker");
    let kept: u64 = rig
        .sink
        .texts()
        .iter()
        .map(|text| u64::try_from(text.len() + 1).unwrap())
        .sum();
    assert_eq!(
        truncated + kept,
        900,
        "every byte is either dropped or delivered"
    );
    assert_eq!(
        rig.sink.texts().first().map(String::as_str),
        Some("line-078")
    );
    assert_eq!(
        rig.sink.texts().last().map(String::as_str),
        Some("line-099")
    );
    assert_eq!(drain.accepted_end(), 900);
}

#[test]
fn resuming_after_a_freed_prefix_skips_to_the_first_newline_and_marks_the_batch() {
    let rig = Rig::new(16);
    let mut data = String::new();
    for n in 0..20 {
        data.push_str(&format!("line-{n:03}\n"));
    }
    rig.write(data.as_bytes());
    let mut before = rig.start(&SupervisorConfig::default());
    rig.pump_all(&mut before);
    assert_eq!(rig.freed(), Some(176));
    drop(before);
    rig.sink.batches.lock().expect("not poisoned").clear();
    rig.write(b"line-020\nline-021\n");

    let mut after = rig.resume(&SupervisorConfig::default());
    rig.pump_all(&mut after);
    let mut again = Vec::new();
    again.extend(rig.sink.batches());

    // Offset 176 is inside `line-019`, so its rest is dropped.
    assert_eq!(rig.sink.texts(), ["line-020", "line-021"]);
    assert!(again[0].resumed);
    assert!(again.iter().skip(1).all(|batch| !batch.resumed));
}

#[test]
fn resuming_a_spool_that_was_never_freed_starts_at_offset_zero_and_keeps_the_first_line() {
    let rig = Rig::new(16);
    rig.write(b"first\nsecond\n");

    let mut drain = rig.resume(&SupervisorConfig::default());
    rig.pump_all(&mut drain);

    assert_eq!(rig.sink.texts(), ["first", "second"]);
    assert!(rig.sink.batches()[0].resumed);
}

#[test]
fn resuming_a_spool_with_nothing_left_waits_for_new_lines() {
    let rig = Rig::new(16);
    rig.write(&b"0123456789abcde\n".repeat(2));
    let mut first = rig.start(&SupervisorConfig::default());
    rig.pump_all(&mut first);
    assert_eq!(rig.freed(), None);
    drop(first);
    rig.sink.batches.lock().expect("not poisoned").clear();

    let mut drain = rig.resume(&SupervisorConfig::default());
    rig.pump_all(&mut drain);

    assert!(rig.sink.batches().is_empty());
    assert_eq!(drain.read_offset(), 32);
}

#[test]
fn finish_hands_over_the_unfinished_line_as_partial_and_frees_it() {
    let rig = Rig::new(16);
    let mut drain = rig.start(&SupervisorConfig::default());
    rig.write(b"done\nno newline here");

    let outcome = drain.finish(&rig.ctx()).expect("the spool can be read");

    assert_eq!(outcome, DrainOutcome::CaughtUp);
    let lines = rig.sink.lines();
    assert_eq!(lines.len(), 2);
    assert!(!lines[0].partial);
    assert_eq!(
        (lines[1].text.as_str(), lines[1].partial),
        ("no newline here", true)
    );
    assert_eq!(drain.accepted_end(), 20);
    assert_eq!(rig.freed(), Some(16));
}

#[test]
fn finish_with_a_busy_sink_keeps_the_rest_and_carries_on_when_called_again() {
    let rig = Rig::new(16);
    let mut drain = rig.start(&SupervisorConfig::default());
    rig.write(b"a\nb");
    rig.sink.busy.store(true, Ordering::SeqCst);

    assert_eq!(drain.finish(&rig.ctx()).expect("reads"), DrainOutcome::Busy);
    rig.sink.busy.store(false, Ordering::SeqCst);
    assert_eq!(
        drain.finish(&rig.ctx()).expect("reads"),
        DrainOutcome::CaughtUp
    );

    assert_eq!(rig.sink.texts(), ["a", "b"]);
    assert!(rig.sink.lines()[1].partial);
}

#[test]
fn a_failure_to_free_the_spool_does_not_stop_the_lines() {
    let rig = Rig::new(16);
    rig.fake.fail_always(
        ProcessOp::ReleaseBelow,
        ProcessError::io("fcntl", None, &std::io::Error::other("no punch")),
    );
    let mut drain = rig.start(&SupervisorConfig::default());
    rig.write(b"one\n");
    rig.pump_all(&mut drain);
    rig.write(b"two\n");
    rig.pump_all(&mut drain);

    assert_eq!(rig.sink.texts(), ["one", "two"]);
}

#[test]
fn a_sink_set_with_the_tail_sink_keeps_the_excerpt_and_the_recorder_sees_every_line() {
    let rig = Rig::new(16);
    let tail = std::sync::Arc::new(TailSink::new(3));
    let recorder = std::sync::Arc::new(Recorder::default());
    let set = SinkSet::new(vec![tail.clone(), recorder.clone()]);
    let mut drain = rig.start(&SupervisorConfig::default());
    rig.write(b"a\nb\nc\nd\n");
    let ctx = DrainContext::new(rig.instance, &rig.redactor, &set, &rig.clock);

    while drain.pump(&ctx).expect("reads") == DrainOutcome::More {}

    assert_eq!(recorder.texts(), ["a", "b", "c", "d"]);
    assert_eq!(
        tail.tail(rig.instance, OutputStream::Stdout),
        ["b", "c", "d"]
    );
    assert_eq!(tail.excerpt(rig.instance), ["b", "c", "d"]);
}
