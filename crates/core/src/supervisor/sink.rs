//! Where redacted log lines go (spec 0010, *Spool log*): the [`LogSink`]
//! port, the [`SinkSet`] that fans a batch out to several sinks, and the
//! [`TailSink`] that keeps the last lines of every stream in memory for
//! failure excerpts.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::Timestamp;
use stackly_domain::{InstanceId, Pid, ProcessStart};
use stackly_platform::OutputStream;

use super::config::{FAILURE_EXCERPT_LINE_CHARS, FAILURE_EXCERPT_LINES, TAIL_LINES};

/// One piece of a log line. The text is already redacted and holds no
/// newline.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogLine {
    /// The redacted text, without the line ending.
    pub text: String,
    /// The process had not ended the line yet when the reader had to hand
    /// it over, because it passed the hard limit or the process exited.
    pub partial: bool,
    /// This piece continues the line of the piece before it, because that
    /// line was longer than the limit of one piece.
    pub continued: bool,
    /// The spool offset right after the logical line this piece belongs
    /// to (after the cut for a piece cut at the hard limit), so a durable
    /// sink can drop a line it already stored when a reader starts again
    /// after a daemon crash (spec 0027, *Buang duplikat*). The pieces of
    /// one logical line share it. `None` for output that never went
    /// through a spool, such as an init step.
    pub end: Option<u64>,
}

impl LogLine {
    /// Returns a whole line: neither `partial` nor `continued`, with no
    /// spool offset.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            partial: false,
            continued: false,
            end: None,
        }
    }
}

/// The process whose spool a batch was read from: the PID and start time
/// of its compound identity (spec 0010). Two runs never share both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpoolSource {
    /// The PID of the main process.
    pub pid: Pid,
    /// When the main process started, to the microsecond.
    pub started_at: ProcessStart,
}

impl SpoolSource {
    /// Returns the source of the process `pid` that started at
    /// `started_at`.
    #[must_use]
    pub const fn new(pid: Pid, started_at: ProcessStart) -> Self {
        Self { pid, started_at }
    }
}

/// Something that happened to the main process of an instance, which a
/// durable sink records as a marker between its lines (spec 0027, AC-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunEvent {
    /// A new main process was spawned, before it ran its first
    /// instruction.
    Started {
        /// Its PID.
        pid: Pid,
        /// When the daemon saw it, from its clock.
        at: Timestamp,
    },
    /// The main process exited, after every batch of its run reached the
    /// sinks.
    Exited {
        /// Its exit code, when it exited on its own.
        code: Option<i32>,
        /// The signal that ended it.
        signal: Option<i32>,
        /// When the daemon saw it, from its clock.
        at: Timestamp,
    },
    /// The daemon adopted a process whose spool could not be opened, so the
    /// output written while no daemon read it is lost.
    Unavailable {
        /// When the daemon saw it, from its clock.
        at: Timestamp,
    },
}

impl RunEvent {
    /// Returns when the daemon saw the event.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        match self {
            Self::Started { at, .. } | Self::Exited { at, .. } | Self::Unavailable { at } => *at,
        }
    }
}

/// What the spool reader hands a [`LogSink`] in one call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogBatch {
    /// The stream the lines came from.
    pub stream: OutputStream,
    /// When the daemon read the lines, from its clock, not when the process
    /// wrote them.
    pub read_at: Timestamp,
    /// The lines, oldest first.
    pub lines: Vec<LogLine>,
    /// The reader picked this spool up again after a daemon restart, so a
    /// few lines at the start may be ones the sink saw before.
    pub resumed: bool,
    /// Bytes dropped unread before the first line because the backlog went
    /// over its limit.
    pub truncated: Option<u64>,
    /// The process whose spool the lines came from; `None` for output that
    /// never went through a spool.
    pub source: Option<SpoolSource>,
    /// On the first batch of a reader that started again after a daemon
    /// restart: the spool offset it started at, the first byte that was
    /// not freed.
    pub resumed_from: Option<u64>,
    /// The init step whose output this is, by its number in the plan; only
    /// for the output of a failed init step.
    pub step: Option<String>,
}

impl LogBatch {
    /// Returns a batch of `lines` read at `read_at`, neither `resumed` nor
    /// `truncated`.
    #[must_use]
    pub const fn new(stream: OutputStream, read_at: Timestamp, lines: Vec<LogLine>) -> Self {
        Self {
            stream,
            read_at,
            lines,
            resumed: false,
            truncated: None,
            source: None,
            resumed_from: None,
            step: None,
        }
    }

    /// Marks the batch as the first one after a daemon restart.
    #[must_use]
    pub const fn resumed(mut self) -> Self {
        self.resumed = true;
        self
    }

    /// Records that `bytes` were dropped before the first line.
    #[must_use]
    pub const fn truncated(mut self, bytes: u64) -> Self {
        self.truncated = Some(bytes);
        self
    }

    /// Records the process whose spool the lines came from.
    #[must_use]
    pub const fn with_source(mut self, source: SpoolSource) -> Self {
        self.source = Some(source);
        self
    }

    /// Returns the bytes of text the batch carries.
    #[must_use]
    pub fn text_bytes(&self) -> usize {
        self.lines.iter().map(|line| line.text.len()).sum()
    }
}

/// How a [`LogSink`] answered a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SinkOutcome {
    /// The sink has the batch; the reader may free its bytes.
    Accepted,
    /// The sink could not take the batch now. The reader keeps it and
    /// offers the same batch again at the next tick; nothing is freed.
    Busy,
}

/// A place log batches go. It must never block: a sink that cannot keep up
/// answers [`SinkOutcome::Busy`], and the instance's process is never held
/// back by it.
pub trait LogSink: Send + Sync {
    /// Offers a batch of one stream of `instance`.
    fn accept(&self, instance: InstanceId, batch: LogBatch) -> SinkOutcome;

    /// Reports something that happened to the main process of `instance`.
    /// It never blocks and has no answer; a sink that does not record runs
    /// ignores it.
    fn run_event(&self, instance: InstanceId, event: RunEvent) {
        let _ = (instance, event);
    }
}

/// Several sinks behind one [`LogSink`].
///
/// A batch goes to every sink. The set answers [`SinkOutcome::Accepted`]
/// only when every sink accepted; when one is busy it answers
/// [`SinkOutcome::Busy`], and when the reader offers the same batch again,
/// the sinks that already accepted it are skipped, so none sees a batch
/// twice.
#[derive(Default)]
pub struct SinkSet {
    sinks: Vec<Arc<dyn LogSink>>,
    delivered: Mutex<HashMap<(InstanceId, OutputStream), Vec<bool>>>,
}

impl SinkSet {
    /// Returns a set over `sinks`.
    #[must_use]
    pub fn new(sinks: Vec<Arc<dyn LogSink>>) -> Self {
        Self {
            sinks,
            delivered: Mutex::new(HashMap::new()),
        }
    }

    /// Adds a sink to the set.
    pub fn push(&mut self, sink: Arc<dyn LogSink>) {
        self.sinks.push(sink);
    }

    /// Returns how many sinks the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sinks.len()
    }

    /// Returns whether the set holds no sink.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }
}

impl LogSink for SinkSet {
    fn accept(&self, instance: InstanceId, batch: LogBatch) -> SinkOutcome {
        let key = (instance, batch.stream);
        let mut done = self
            .delivered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key)
            .unwrap_or_default();
        done.resize(self.sinks.len(), false);
        let mut all = true;
        for (sink, seen) in self.sinks.iter().zip(done.iter_mut()) {
            if !*seen {
                *seen = sink.accept(instance, batch.clone()) == SinkOutcome::Accepted;
            }
            all &= *seen;
        }
        if all {
            SinkOutcome::Accepted
        } else {
            self.delivered
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key, done);
            SinkOutcome::Busy
        }
    }

    fn run_event(&self, instance: InstanceId, event: RunEvent) {
        for sink in &self.sinks {
            sink.run_event(instance, event);
        }
    }
}

impl std::fmt::Debug for SinkSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SinkSet")
            .field("sinks", &self.sinks.len())
            .finish_non_exhaustive()
    }
}

/// The last lines of each stream of each instance, in memory, already
/// redacted: the source of the excerpt in a failure (spec 0010,
/// `lastFailure.excerpt`). It never answers [`SinkOutcome::Busy`].
///
/// Each line is kept up to [`FAILURE_EXCERPT_LINE_CHARS`] characters; the
/// pieces that continue a long line are not kept, since they start after
/// that cut.
#[derive(Debug)]
pub struct TailSink {
    capacity: usize,
    tails: Mutex<HashMap<InstanceId, [VecDeque<String>; 2]>>,
}

impl TailSink {
    /// Returns a tail sink that keeps `capacity` lines per stream per
    /// instance.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            tails: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<InstanceId, [VecDeque<String>; 2]>> {
        self.tails.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns the kept lines of one stream, oldest first.
    #[must_use]
    pub fn tail(&self, instance: InstanceId, stream: OutputStream) -> Vec<String> {
        self.lock()
            .get(&instance)
            .map(|tails| tails[slot(stream)].iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns the failure excerpt of `instance`: the last lines of stderr
    /// first, then, if there is room, the last lines of stdout, at most
    /// [`FAILURE_EXCERPT_LINES`] in all.
    #[must_use]
    pub fn excerpt(&self, instance: InstanceId) -> Vec<String> {
        self.excerpt_of(instance, FAILURE_EXCERPT_LINES)
    }

    /// Like [`excerpt`](Self::excerpt) with a limit of `lines` lines.
    #[must_use]
    pub fn excerpt_of(&self, instance: InstanceId, lines: usize) -> Vec<String> {
        let mut out = self.tail(instance, OutputStream::Stderr);
        let skip = out.len().saturating_sub(lines);
        out.drain(..skip);
        let room = lines - out.len();
        let stdout = self.tail(instance, OutputStream::Stdout);
        let from = stdout.len().saturating_sub(room);
        out.extend(stdout.into_iter().skip(from));
        out
    }

    /// Forgets everything kept for `instance`, when it starts a new run or
    /// is deleted.
    pub fn forget(&self, instance: InstanceId) {
        self.lock().remove(&instance);
    }
}

impl Default for TailSink {
    fn default() -> Self {
        Self::new(TAIL_LINES)
    }
}

impl LogSink for TailSink {
    fn accept(&self, instance: InstanceId, batch: LogBatch) -> SinkOutcome {
        if self.capacity == 0 {
            return SinkOutcome::Accepted;
        }
        let mut tails = self.lock();
        let tail = &mut tails.entry(instance).or_default()[slot(batch.stream)];
        for line in batch.lines.into_iter().filter(|line| !line.continued) {
            if tail.len() == self.capacity {
                tail.pop_front();
            }
            tail.push_back(line.text.chars().take(FAILURE_EXCERPT_LINE_CHARS).collect());
        }
        SinkOutcome::Accepted
    }
}

const fn slot(stream: OutputStream) -> usize {
    match stream {
        OutputStream::Stdout => 0,
        OutputStream::Stderr => 1,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;

    fn batch(stream: OutputStream, lines: &[&str]) -> LogBatch {
        LogBatch::new(
            stream,
            Timestamp::UNIX_EPOCH,
            lines.iter().map(|text| LogLine::new(*text)).collect(),
        )
    }

    struct Counting {
        busy: AtomicBool,
        accepted: AtomicUsize,
    }

    impl Counting {
        fn new(busy: bool) -> Arc<Self> {
            Arc::new(Self {
                busy: AtomicBool::new(busy),
                accepted: AtomicUsize::new(0),
            })
        }
    }

    impl LogSink for Counting {
        fn accept(&self, _instance: InstanceId, _batch: LogBatch) -> SinkOutcome {
            if self.busy.load(Ordering::SeqCst) {
                return SinkOutcome::Busy;
            }
            self.accepted.fetch_add(1, Ordering::SeqCst);
            SinkOutcome::Accepted
        }
    }

    #[test]
    fn the_tail_keeps_only_the_newest_lines() {
        let tail = TailSink::new(3);
        let id = InstanceId::generate();

        tail.accept(id, batch(OutputStream::Stdout, &["a", "b", "c", "d", "e"]));

        assert_eq!(tail.tail(id, OutputStream::Stdout), ["c", "d", "e"]);
        assert!(tail.tail(id, OutputStream::Stderr).is_empty());
    }

    #[test]
    fn the_excerpt_lists_stderr_before_stdout_within_the_limit() {
        let tail = TailSink::new(64);
        let id = InstanceId::generate();
        tail.accept(id, batch(OutputStream::Stdout, &["o1", "o2", "o3"]));
        tail.accept(id, batch(OutputStream::Stderr, &["e1", "e2"]));

        assert_eq!(tail.excerpt_of(id, 4), ["e1", "e2", "o2", "o3"]);
        assert_eq!(tail.excerpt_of(id, 1), ["e2"]);
    }

    #[test]
    fn a_tail_line_is_cut_and_continuations_are_not_kept() {
        let tail = TailSink::new(8);
        let id = InstanceId::generate();
        let mut cont = LogLine::new("rest");
        cont.continued = true;
        let mut lines = batch(OutputStream::Stdout, &[]);
        lines.lines = vec![LogLine::new("é".repeat(300)), cont];

        tail.accept(id, lines);

        let kept = tail.tail(id, OutputStream::Stdout);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].chars().count(), FAILURE_EXCERPT_LINE_CHARS);
    }

    #[test]
    fn forgetting_an_instance_clears_its_tail() {
        let tail = TailSink::default();
        let id = InstanceId::generate();
        tail.accept(id, batch(OutputStream::Stderr, &["x"]));

        tail.forget(id);

        assert!(tail.excerpt(id).is_empty());
    }

    #[test]
    fn a_busy_sink_makes_the_set_busy_and_a_retry_skips_the_sinks_that_accepted() {
        let calm = Counting::new(false);
        let slow = Counting::new(true);
        let set = SinkSet::new(vec![calm.clone(), slow.clone()]);
        let id = InstanceId::generate();

        assert_eq!(
            set.accept(id, batch(OutputStream::Stdout, &["x"])),
            SinkOutcome::Busy
        );
        slow.busy.store(false, Ordering::SeqCst);
        assert_eq!(
            set.accept(id, batch(OutputStream::Stdout, &["x"])),
            SinkOutcome::Accepted
        );

        assert_eq!(calm.accepted.load(Ordering::SeqCst), 1);
        assert_eq!(slow.accepted.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_empty_set_accepts_everything() {
        let set = SinkSet::default();

        assert!(set.is_empty());
        assert_eq!(
            set.accept(InstanceId::generate(), batch(OutputStream::Stdout, &["x"])),
            SinkOutcome::Accepted
        );
    }
}
