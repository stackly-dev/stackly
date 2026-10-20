//! The daemon's log writer (spec 0006, *Log daemon*).
//!
//! The `tracing` subscriber never writes anywhere itself: its writer,
//! [`QueueWriter`], hands each JSON line to a bounded queue with
//! `try_send`, so logging never blocks the daemon. One writer thread owns
//! stderr and, once [`LogSink::install_file`] ran after the daemon lock was
//! taken, a [`RotatingFile`] under `<logs>/daemon/`. A daemon that fails to
//! take the lock never installs the file, so it only ever writes to stderr.
//!
//! A line is dropped, and counted, when the queue is full or when writing
//! it to stderr or to the installed file fails. A failed file is opened
//! again on the first line after [`LogSinkConfig::reopen_delay`]. After a
//! run of queue drops, the writer logs one `log lines dropped` line with
//! the count. Any drop or file failure in the last minute marks the
//! `log_sink` subsystem degraded.

mod redacting;
mod rotating;
mod window;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_platform::{Clock, SystemClock};
use stackly_protocol::daemon::{SubsystemHealth, SubsystemStatus, subsystem};
use stackly_protocol::{Count, Timestamp};
use tracing_subscriber::fmt::MakeWriter;

pub use redacting::{RedactingMakeWriter, RedactingWriter};
pub use rotating::RotatingFile;
use window::ProblemWindow;

/// File name of the daemon log under `<logs>/daemon/`.
pub const LOG_FILE_NAME: &str = "stacklyd.log";
/// Size at which the daemon log rotates: 20 MiB.
pub const LOG_FILE_MAX: u64 = 20 * 1024 * 1024;
/// How many daemon log files are kept, `stacklyd.log` to `stacklyd.log.4`.
pub const LOG_FILES_KEPT: usize = 5;
/// How many lines the queue holds before new ones are dropped.
pub const LOG_QUEUE_LEN: usize = 8192;
/// How long a failed log file stays closed before the next line opens it
/// again.
pub const LOG_REOPEN_DELAY: Duration = Duration::from_secs(10);
/// How long a drop or file failure keeps `log_sink` degraded.
pub const LOG_PROBLEM_WINDOW: Duration = Duration::from_secs(60);
/// How long [`LogSink::flush`] and [`LogSink::install_file`] wait for the
/// writer thread.
pub const LOG_CONTROL_TIMEOUT: Duration = Duration::from_secs(1);

/// The `target` of the lines the writer thread writes itself.
const TARGET: &str = "stacklyd::log_sink";
/// How long a control message waits between tries while the queue is full.
const CONTROL_RETRY: Duration = Duration::from_millis(5);

/// A place the writer thread writes whole lines to, after stderr.
pub trait LineSink: Send {
    /// Writes one line, newline included.
    ///
    /// # Errors
    ///
    /// Returns the OS error; the writer then drops the line and closes the
    /// sink until it is opened again.
    fn write_line(&mut self, line: &[u8]) -> io::Result<()>;

    /// Flushes buffered data, if any.
    ///
    /// # Errors
    ///
    /// Returns the OS error.
    fn flush(&mut self) -> io::Result<()>;
}

/// Opens the file sink, the first time and after every failure.
pub type SinkOpener = Box<dyn FnMut() -> io::Result<Box<dyn LineSink>> + Send>;

/// Tunables of the log writer. The daemon uses [`LogSinkConfig::DAEMON`];
/// tests pick a small queue or a short reopen delay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogSinkConfig {
    /// How many lines the queue holds.
    pub queue_len: usize,
    /// How long a failed file stays closed.
    pub reopen_delay: Duration,
}

impl LogSinkConfig {
    /// The daemon's values: [`LOG_QUEUE_LEN`] and [`LOG_REOPEN_DELAY`].
    pub const DAEMON: Self = Self {
        queue_len: LOG_QUEUE_LEN,
        reopen_delay: LOG_REOPEN_DELAY,
    };
}

/// The writer thread did not take a control message in time, because the
/// queue stayed full or the thread is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the log writer did not take the request in time")]
pub struct WriterBusy;

/// The handle to the log writer thread.
///
/// Dropping it does not stop the thread: the thread ends once every
/// [`QueueWriter`] is gone too. The daemon's global subscriber keeps one
/// for the life of the process, so the daemon calls [`LogSink::flush`]
/// before it exits.
#[derive(Debug)]
pub struct LogSink {
    tx: SyncSender<Command>,
    stats: Arc<LogStats>,
}

impl LogSink {
    /// Starts the writer thread, which writes every line to `stderr` until
    /// a file is installed.
    ///
    /// # Errors
    ///
    /// Returns the OS error when the thread cannot be spawned.
    pub fn start(stderr: Box<dyn Write + Send>, config: LogSinkConfig) -> io::Result<Self> {
        let (tx, rx) = sync_channel(config.queue_len);
        let stats = Arc::new(LogStats::new());
        let writer = Writer {
            rx,
            stderr,
            file: None,
            stats: Arc::clone(&stats),
            reopen_delay: config.reopen_delay,
            reported_queue_drops: 0,
        };
        thread::Builder::new()
            .name("stacklyd-log".to_owned())
            .spawn(move || writer.run())?;
        Ok(Self { tx, stats })
    }

    /// Returns a writer for the `tracing` subscriber.
    #[must_use]
    pub fn make_writer(&self) -> QueueWriter {
        QueueWriter {
            tx: self.tx.clone(),
            stats: Arc::clone(&self.stats),
        }
    }

    /// Installs the daemon log file, `dir/stacklyd.log` rotating at
    /// [`LOG_FILE_MAX`] with [`LOG_FILES_KEPT`] files. Lines queued before
    /// this call go to stderr only, lines after it to both. The daemon
    /// calls it once, after it took the lock.
    ///
    /// # Errors
    ///
    /// Returns [`WriterBusy`] when the writer did not take the request
    /// within [`LOG_CONTROL_TIMEOUT`]; `log_sink` then shows degraded.
    pub fn install_file(&self, dir: &Path) -> Result<(), WriterBusy> {
        let dir: PathBuf = dir.to_path_buf();
        self.install_sink(Box::new(move || {
            RotatingFile::open(&dir, LOG_FILE_NAME, LOG_FILE_MAX, LOG_FILES_KEPT)
                .map(|file| Box::new(file) as Box<dyn LineSink>)
        }))
    }

    /// Installs a file sink through `opener`, which the writer calls right
    /// away and again after each failure. [`LogSink::install_file`] is the
    /// daemon's opener; tests pass one that fails on purpose.
    ///
    /// # Errors
    ///
    /// Returns [`WriterBusy`] when the writer did not take the request
    /// within [`LOG_CONTROL_TIMEOUT`]; `log_sink` then shows degraded.
    pub fn install_sink(&self, opener: SinkOpener) -> Result<(), WriterBusy> {
        let deadline = Instant::now() + LOG_CONTROL_TIMEOUT;
        let sent = self.send_control(Command::InstallFile(opener), deadline);
        if sent.is_err() {
            self.stats.record_file_failure(Instant::now());
        }
        sent
    }

    /// Waits until the writer wrote every line queued before this call, at
    /// most `timeout`, and returns whether it did.
    #[must_use = "a false result means some lines may not be written"]
    pub fn flush(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (ack_tx, ack_rx) = sync_channel(1);
        if self.send_control(Command::Flush(ack_tx), deadline).is_err() {
            return false;
        }
        ack_rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    }

    /// Returns how many lines were dropped since the writer started.
    #[must_use]
    pub fn dropped(&self) -> Count {
        Count::saturating(self.stats.dropped.load(Ordering::Relaxed))
    }

    /// Returns the `log_sink` subsystem health at `now`: `degraded` when a
    /// line was dropped or the file failed within [`LOG_PROBLEM_WINDOW`],
    /// else `ok`.
    #[must_use]
    pub fn health(&self, now: Instant) -> SubsystemHealth {
        let (drops, file_failed) = self.stats.recent(now);
        let (status, detail) = if drops > 0 {
            let lines = if drops == 1 { "line" } else { "lines" };
            (
                SubsystemStatus::Degraded,
                Some(format!("{drops} log {lines} dropped in the last minute")),
            )
        } else if file_failed {
            (
                SubsystemStatus::Degraded,
                Some("the log file could not be written in the last minute".to_owned()),
            )
        } else {
            (SubsystemStatus::Ok, None)
        };
        SubsystemHealth {
            name: subsystem::LOG_SINK.to_owned(),
            status,
            detail,
        }
    }

    fn send_control(&self, mut command: Command, deadline: Instant) -> Result<(), WriterBusy> {
        loop {
            match self.tx.try_send(command) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Disconnected(_)) => return Err(WriterBusy),
                Err(TrySendError::Full(back)) => {
                    if Instant::now() >= deadline {
                        return Err(WriterBusy);
                    }
                    command = back;
                    thread::sleep(CONTROL_RETRY);
                }
            }
        }
    }
}

/// The `tracing` writer: each event's line goes to the queue with
/// `try_send` when the per event writer drops, so it never waits.
#[derive(Debug, Clone)]
pub struct QueueWriter {
    tx: SyncSender<Command>,
    stats: Arc<LogStats>,
}

impl<'a> MakeWriter<'a> for QueueWriter {
    type Writer = LineWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LineWriter {
            queue: self,
            line: Vec::new(),
        }
    }
}

/// Collects one event's line and queues it when dropped.
#[derive(Debug)]
pub struct LineWriter<'a> {
    queue: &'a QueueWriter,
    line: Vec<u8>,
}

impl Write for LineWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.line.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LineWriter<'_> {
    fn drop(&mut self) {
        if self.line.is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.line);
        if self.queue.tx.try_send(Command::Line(line)).is_err() {
            self.queue.stats.record_queue_drop(Instant::now());
        }
    }
}

enum Command {
    Line(Vec<u8>),
    InstallFile(SinkOpener),
    Flush(SyncSender<()>),
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Line(_) => "Line",
            Self::InstallFile(_) => "InstallFile",
            Self::Flush(_) => "Flush",
        })
    }
}

/// Counters shared by the queue writers and the writer thread.
#[derive(Debug)]
struct LogStats {
    /// Every dropped line, from the queue or from a failed write.
    dropped: AtomicU64,
    /// Lines dropped because the queue was full.
    queue_dropped: AtomicU64,
    window: Mutex<ProblemWindow>,
}

impl LogStats {
    fn new() -> Self {
        Self {
            dropped: AtomicU64::new(0),
            queue_dropped: AtomicU64::new(0),
            window: Mutex::new(ProblemWindow::new(Instant::now(), LOG_PROBLEM_WINDOW)),
        }
    }

    fn record_queue_drop(&self, now: Instant) {
        self.queue_dropped.fetch_add(1, Ordering::Relaxed);
        self.record_write_drop(now);
    }

    fn record_write_drop(&self, now: Instant) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
        self.window().record_drop(now);
    }

    fn record_file_failure(&self, now: Instant) {
        self.window().record_failure(now);
    }

    fn recent(&self, now: Instant) -> (u64, bool) {
        let window = self.window();
        (window.drops(now), window.failed(now))
    }

    fn window(&self) -> std::sync::MutexGuard<'_, ProblemWindow> {
        self.window.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The installed file sink, open or waiting to be opened again.
struct FileSlot {
    opener: SinkOpener,
    sink: Option<Box<dyn LineSink>>,
    retry_at: Option<Instant>,
}

struct Writer {
    rx: Receiver<Command>,
    stderr: Box<dyn Write + Send>,
    file: Option<FileSlot>,
    stats: Arc<LogStats>,
    reopen_delay: Duration,
    reported_queue_drops: u64,
}

impl Writer {
    fn run(mut self) {
        while let Ok(command) = self.rx.recv() {
            match command {
                Command::Line(line) => {
                    self.report_queue_drops();
                    self.write(&line);
                }
                Command::InstallFile(opener) => {
                    self.file = Some(FileSlot {
                        opener,
                        sink: None,
                        retry_at: None,
                    });
                    // Open right away, so a failure shows before the first
                    // line arrives.
                    self.ensure_open(Instant::now());
                }
                Command::Flush(ack) => {
                    self.report_queue_drops();
                    let _ = self.stderr.flush();
                    if let Some(sink) = self.file.as_mut().and_then(|slot| slot.sink.as_mut()) {
                        let _ = sink.flush();
                    }
                    let _ = ack.send(());
                }
            }
        }
    }

    /// Writes one line to stderr and the file; a sink that fails drops the
    /// line, which counts once.
    fn write(&mut self, line: &[u8]) {
        let mut lost = self.stderr.write_all(line).is_err();
        let now = Instant::now();
        if self.file.is_some() {
            let written = match self.ensure_open(now) {
                Some(sink) => sink.write_line(line),
                None => Err(io::Error::other("the log file is closed")),
            };
            if let Err(error) = written {
                lost = true;
                if let Some(slot) = self.file.as_mut().filter(|slot| slot.sink.is_some()) {
                    slot.sink = None;
                    slot.retry_at = Some(now + self.reopen_delay);
                    self.stats.record_file_failure(now);
                    self.announce_file_failure(&error);
                }
            }
        }
        if lost {
            self.stats.record_write_drop(now);
        }
    }

    /// Returns the open file sink, opening it first when it is closed and
    /// its retry time has come.
    fn ensure_open(&mut self, now: Instant) -> Option<&mut Box<dyn LineSink>> {
        let slot = self.file.as_mut()?;
        if slot.sink.is_none() && slot.retry_at.is_none_or(|at| now >= at) {
            match (slot.opener)() {
                Ok(sink) => {
                    slot.sink = Some(sink);
                    slot.retry_at = None;
                }
                Err(error) => {
                    slot.retry_at = Some(now + self.reopen_delay);
                    self.stats.record_file_failure(now);
                    self.announce_file_failure(&error);
                    return None;
                }
            }
        }
        self.file.as_mut()?.sink.as_mut()
    }

    /// After queue drops, writes one line with how many were dropped.
    fn report_queue_drops(&mut self) {
        let total = self.stats.queue_dropped.load(Ordering::Relaxed);
        if total > self.reported_queue_drops {
            let count = total - self.reported_queue_drops;
            self.reported_queue_drops = total;
            let line = own_line("WARN", "log lines dropped", json!({ "count": count }));
            self.write(&line);
        }
    }

    /// Tells stderr (only) that the file failed; the file itself is the
    /// sink that failed.
    fn announce_file_failure(&mut self, error: &io::Error) {
        let line = own_line(
            "WARN",
            "cannot write the log file, retrying later",
            json!({
                "error": error.to_string(),
                "retry_in_seconds": self.reopen_delay.as_secs(),
            }),
        );
        if self.stderr.write_all(&line).is_err() {
            self.stats.record_write_drop(Instant::now());
        }
    }
}

/// A line the writer thread writes itself, in the subscriber's JSON shape.
fn own_line(level: &str, message: &str, fields: serde_json::Value) -> Vec<u8> {
    let mut all = serde_json::Map::new();
    all.insert("message".to_owned(), message.into());
    if let serde_json::Value::Object(extra) = fields {
        all.extend(extra);
    }
    let value = json!({
        "timestamp": Timestamp::from_jiff(SystemClock.now()).to_string(),
        "level": level,
        "fields": all,
        "target": TARGET,
    });
    let mut line = value.to_string().into_bytes();
    line.push(b'\n');
    line
}

#[cfg(test)]
mod tests;
