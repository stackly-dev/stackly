use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Condvar, MutexGuard};

use stackly_testkit::TestRoot;

use super::*;

/// Generous bound for work that should take microseconds, so a slow test
/// machine does not make the tests flaky.
const QUICK: Duration = Duration::from_secs(2);

/// An in memory stderr the test can read.
#[derive(Debug, Clone, Default)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl SharedBuf {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    fn lines(&self) -> Vec<String> {
        self.text().lines().map(str::to_owned).collect()
    }
}

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A stderr whose writes block until the test opens the gate, like a pipe
/// nobody reads.
#[derive(Debug, Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>, SharedBuf, Arc<AtomicUsize>);

impl Gate {
    fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    /// How many writes reached the gate.
    fn waiting(&self) -> usize {
        self.2.load(Ordering::SeqCst)
    }

    fn wait_open(&self) -> MutexGuard<'_, bool> {
        let guard = self.0.0.lock().unwrap();
        self.0.1.wait_while(guard, |open| !*open).unwrap()
    }
}

impl Write for Gate {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.2.fetch_add(1, Ordering::SeqCst);
        drop(self.wait_open());
        self.1.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A file sink kept in memory that fails while `fail` is set.
#[derive(Debug, Clone, Default)]
struct FlakySink {
    lines: SharedBuf,
    fail: Arc<AtomicBool>,
    opens: Arc<AtomicUsize>,
}

impl FlakySink {
    fn opener(&self) -> SinkOpener {
        let sink = self.clone();
        Box::new(move || {
            sink.opens.fetch_add(1, Ordering::SeqCst);
            if sink.fail.load(Ordering::SeqCst) {
                Err(io::Error::other("disk on fire"))
            } else {
                Ok(Box::new(sink.clone()) as Box<dyn LineSink>)
            }
        })
    }
}

impl LineSink for FlakySink {
    fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(io::Error::other("disk on fire"));
        }
        self.lines.write_all(line)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn start(stderr: impl Write + Send + 'static, config: LogSinkConfig) -> LogSink {
    LogSink::start(Box::new(stderr), config).unwrap()
}

fn log(writer: &QueueWriter, text: &str) {
    let mut line = writer.make_writer();
    line.write_all(format!("{text}\n").as_bytes()).unwrap();
}

fn flush(sink: &LogSink) {
    assert!(sink.flush(QUICK), "the writer did not flush in time");
}

fn json_lines(text: &str) -> Vec<serde_json::Value> {
    text.lines()
        .filter(|line| line.starts_with('{'))
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + QUICK;
    while !done() {
        assert!(Instant::now() < deadline, "condition not met in time");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn lines_go_to_stderr_until_the_file_is_installed_then_to_both() {
    let root = TestRoot::new().unwrap();
    let stderr = SharedBuf::default();
    let sink = start(stderr.clone(), LogSinkConfig::DAEMON);
    let writer = sink.make_writer();

    log(&writer, "before the lock");
    sink.install_file(root.path()).unwrap();
    log(&writer, "after the lock");
    flush(&sink);

    assert_eq!(stderr.lines(), ["before the lock", "after the lock"]);
    let file = std::fs::read_to_string(root.path().join(LOG_FILE_NAME)).unwrap();
    assert_eq!(file, "after the lock\n");
    assert_eq!(sink.dropped(), Count::ZERO);
    assert_eq!(sink.health(Instant::now()).status, SubsystemStatus::Ok);
}

#[test]
fn the_installed_file_is_private_and_holds_the_lines_logged_after_it() {
    use std::os::unix::fs::PermissionsExt;

    let root = TestRoot::new().unwrap();
    let sink = start(io::sink(), LogSinkConfig::DAEMON);
    sink.install_file(root.path()).unwrap();
    log(&sink.make_writer(), "one line");
    flush(&sink);

    let metadata = std::fs::metadata(root.path().join(LOG_FILE_NAME)).unwrap();

    assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
    assert_eq!(metadata.len(), 9);
}

#[test]
fn a_full_queue_drops_lines_without_blocking_and_reports_the_count() {
    let stderr = Gate::default();
    let config = LogSinkConfig {
        queue_len: 4,
        reopen_delay: LOG_REOPEN_DELAY,
    };
    let sink = start(stderr.clone(), config);
    let writer = sink.make_writer();
    log(&writer, "first");
    wait_until(|| stderr.waiting() == 1);

    let started = Instant::now();
    for number in 0..14 {
        log(&writer, &format!("line {number}"));
    }
    let elapsed = started.elapsed();

    assert!(elapsed < QUICK, "logging blocked for {elapsed:?}");
    assert_eq!(sink.dropped(), Count::saturating(10));
    let health = sink.health(Instant::now());
    assert_eq!(health.name, subsystem::LOG_SINK);
    assert_eq!(health.status, SubsystemStatus::Degraded);
    assert_eq!(
        health.detail.as_deref(),
        Some("10 log lines dropped in the last minute")
    );
    stderr.open();
    flush(&sink);
    let text = stderr.1.text();
    let reports: Vec<_> = json_lines(&text)
        .into_iter()
        .filter(|line| line["fields"]["message"] == "log lines dropped")
        .collect();
    assert_eq!(reports.len(), 1, "{text}");
    assert_eq!(reports[0]["fields"]["count"], 10);
    assert_eq!(reports[0]["level"], "WARN");
    assert_eq!(reports[0]["target"], TARGET);
    let kept: Vec<_> = text.lines().filter(|line| !line.starts_with('{')).collect();
    assert_eq!(kept, ["first", "line 0", "line 1", "line 2", "line 3"]);
    let later = Instant::now() + LOG_PROBLEM_WINDOW + Duration::from_secs(1);
    assert_eq!(sink.health(later).status, SubsystemStatus::Ok);
}

#[test]
fn an_unread_stderr_pipe_never_blocks_the_logger() {
    let (reader, stderr) = io::pipe().unwrap();
    let sink = start(stderr, LogSinkConfig::DAEMON);
    let writer = sink.make_writer();
    let text = "x".repeat(200);

    let started = Instant::now();
    for _ in 0..20_000 {
        log(&writer, &text);
    }
    let elapsed = started.elapsed();

    assert!(elapsed < QUICK, "logging blocked for {elapsed:?}");
    assert!(sink.dropped().get() > 0);
    assert_eq!(
        sink.health(Instant::now()).status,
        SubsystemStatus::Degraded
    );
    // Closing the read end makes every pending write fail instead of block.
    drop(reader);
    flush(&sink);
}

#[test]
fn a_failing_file_drops_the_line_and_is_opened_again_after_the_delay() {
    let stderr = SharedBuf::default();
    let config = LogSinkConfig {
        queue_len: LOG_QUEUE_LEN,
        reopen_delay: Duration::from_millis(200),
    };
    let sink = start(stderr.clone(), config);
    let writer = sink.make_writer();
    let file = FlakySink::default();
    sink.install_sink(file.opener()).unwrap();
    log(&writer, "kept 1");
    flush(&sink);

    file.fail.store(true, Ordering::SeqCst);
    log(&writer, "lost 1");
    flush(&sink);
    file.fail.store(false, Ordering::SeqCst);
    log(&writer, "lost 2 while closed");
    flush(&sink);
    let failed = Instant::now();

    assert_eq!(file.lines.text(), "kept 1\n");
    assert_eq!(file.opens.load(Ordering::SeqCst), 1);
    assert_eq!(sink.dropped(), Count::saturating(2));
    let health = sink.health(failed);
    assert_eq!(health.status, SubsystemStatus::Degraded);
    assert_eq!(
        health.detail.as_deref(),
        Some("2 log lines dropped in the last minute")
    );
    let text = stderr.text();
    assert!(text.contains("lost 1\n") && text.contains("lost 2 while closed\n"));
    let warnings: Vec<_> = json_lines(&text)
        .into_iter()
        .filter(|line| line["fields"]["message"] == "cannot write the log file, retrying later")
        .collect();
    assert_eq!(warnings.len(), 1, "{text}");
    assert_eq!(warnings[0]["fields"]["error"], "disk on fire");

    thread::sleep(Duration::from_millis(250));
    log(&writer, "kept 2");
    flush(&sink);

    assert_eq!(file.opens.load(Ordering::SeqCst), 2);
    assert_eq!(file.lines.text(), "kept 1\nkept 2\n");
    assert_eq!(sink.dropped(), Count::saturating(2));
}

#[test]
fn a_file_that_cannot_be_opened_marks_the_sink_degraded_right_away() {
    let stderr = SharedBuf::default();
    let sink = start(stderr.clone(), LogSinkConfig::DAEMON);
    let file = FlakySink::default();
    file.fail.store(true, Ordering::SeqCst);

    sink.install_sink(file.opener()).unwrap();
    flush(&sink);

    let health = sink.health(Instant::now());
    assert_eq!(health.status, SubsystemStatus::Degraded);
    assert_eq!(
        health.detail.as_deref(),
        Some("the log file could not be written in the last minute")
    );
    assert_eq!(sink.dropped(), Count::ZERO);
    assert!(stderr.text().contains("cannot write the log file"));
    let later = Instant::now() + LOG_PROBLEM_WINDOW + Duration::from_secs(1);
    assert_eq!(sink.health(later).status, SubsystemStatus::Ok);
}

#[test]
fn a_failing_stderr_drops_the_line_but_the_file_still_gets_it() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let sink = start(Broken, LogSinkConfig::DAEMON);
    let file = FlakySink::default();
    sink.install_sink(file.opener()).unwrap();

    log(&sink.make_writer(), "line");
    flush(&sink);

    assert_eq!(file.lines.text(), "line\n");
    assert_eq!(sink.dropped(), Count::saturating(1));
}

#[test]
fn own_lines_have_the_subscriber_shape() {
    let line = own_line("WARN", "log lines dropped", json!({ "count": 3 }));

    assert_eq!(line.last(), Some(&b'\n'));
    let value: serde_json::Value = serde_json::from_slice(&line).unwrap();
    assert_eq!(value["level"], "WARN");
    assert_eq!(value["target"], TARGET);
    assert_eq!(value["fields"]["message"], "log lines dropped");
    assert_eq!(value["fields"]["count"], 3);
    let timestamp = value["timestamp"].as_str().unwrap();
    assert!(timestamp.parse::<Timestamp>().is_ok(), "{timestamp}");
}

#[test]
fn flush_reports_false_when_the_writer_is_stuck() {
    let stderr = Gate::default();
    let config = LogSinkConfig {
        queue_len: 1,
        reopen_delay: LOG_REOPEN_DELAY,
    };
    let sink = start(stderr.clone(), config);
    let writer = sink.make_writer();
    log(&writer, "first");
    wait_until(|| stderr.waiting() == 1);
    log(&writer, "fills the queue");

    let started = Instant::now();
    let flushed = sink.flush(Duration::from_millis(100));

    assert!(!flushed);
    assert!(started.elapsed() < QUICK);
    stderr.open();
    flush(&sink);
}
