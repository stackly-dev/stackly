//! The reader of one spool stream (spec 0010, *Spool log*, AC-12): it turns
//! the raw bytes a process wrote into redacted [`LogLine`]s, offers them to
//! a [`LogSink`], and frees the bytes the sink accepted.
//!
//! One [`SpoolDrain`] reads one stream (`stdout` or `stderr`) of one run.
//! The instance actor owns two of them and calls
//! [`pump`](SpoolDrain::pump) every `SPOOL_DRAIN_INTERVAL` while the main
//! process lives, [`finish`](SpoolDrain::finish) once when it exits, and
//! builds one with [`resume`](SpoolDrain::resume) when the daemon adopts a
//! process (or reads an old spool before a respawn). The reader does plain
//! `pread` and `fcntl` calls on a spool of at most one chunk per tick, so
//! the actor may call it directly; it never waits for a sink.

use std::num::NonZeroU64;

use stackly_domain::InstanceId;
use stackly_platform::{Clock, OutputStream, ProcessError, SpoolReader};

use super::config::SupervisorConfig;
use super::sink::{LogBatch, LogLine, LogSink, SinkOutcome};
use crate::secret::Redactor;

/// What one call of [`SpoolDrain::pump`] or [`SpoolDrain::finish`] left
/// behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DrainOutcome {
    /// Everything the process wrote so far was read.
    CaughtUp,
    /// A whole chunk was read, so there may be more; call again.
    More,
    /// The sink answered [`SinkOutcome::Busy`]. The batch waits inside the
    /// reader and is offered again by the next call; nothing was freed.
    Busy,
}

/// What a drain needs on every call: who the lines belong to, where they
/// go, and the clocks and rules that touch them.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct DrainContext<'a> {
    /// The instance whose process wrote the lines.
    pub instance: InstanceId,
    /// Hides secret values in every whole line before it is cut.
    pub redactor: &'a Redactor,
    /// Receives the batches.
    pub sink: &'a dyn LogSink,
    /// Stamps `read_at` of each batch.
    pub clock: &'a dyn Clock,
}

impl<'a> DrainContext<'a> {
    /// Returns a context for `instance`.
    #[must_use]
    pub fn new(
        instance: InstanceId,
        redactor: &'a Redactor,
        sink: &'a dyn LogSink,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            instance,
            redactor,
            sink,
            clock,
        }
    }
}

/// A batch the sink has not accepted yet, with the spool offset where its
/// last line ends.
struct Pending {
    batch: LogBatch,
    end: u64,
}

/// Bytes the reader throws away instead of reading.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Skip {
    /// Nothing is skipped.
    No,
    /// Up to and including the next newline, after a start in the middle
    /// of the spool.
    ToNewline,
    /// Like [`ToNewline`](Self::ToNewline), after the backlog limit
    /// dropped the oldest bytes; the skipped bytes count as truncated.
    ToNewlineTruncated,
}

/// The reader of one stream of one run.
pub struct SpoolDrain {
    stream: OutputStream,
    reader: Box<dyn SpoolReader>,
    read_chunk: usize,
    line_max: usize,
    hard_max: usize,
    backlog_max: u64,
    /// The next byte to read.
    read_offset: u64,
    /// Bytes read that do not end a line yet; they start at `held_start`.
    held: Vec<u8>,
    held_start: u64,
    skip: Skip,
    /// The next piece continues a line the reader already handed over in
    /// part.
    continuing: bool,
    /// Where the last piece handed to the sink ends.
    line_end: u64,
    /// Where the last line the sink accepted ends.
    accepted_end: u64,
    pending: Option<Pending>,
    resumed: bool,
    truncated: u64,
    release_failed: bool,
    buf: Vec<u8>,
}

impl SpoolDrain {
    /// Returns a reader for a spool the process just started writing, from
    /// offset 0. `config` holds the chunk, line, and backlog limits.
    #[must_use]
    pub fn start(
        stream: OutputStream,
        reader: Box<dyn SpoolReader>,
        config: &SupervisorConfig,
    ) -> Self {
        Self::at(stream, reader, config, 0, Skip::No, false)
    }

    /// Returns a reader that carries on with an existing spool after a
    /// daemon restart: it starts at the first byte that is not freed (the
    /// end when nothing is left), skips to the first newline unless that
    /// offset is 0, and marks the first batch `resumed`. It also reads an
    /// old spool to the end before a respawn.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when the spool length or first data
    /// offset cannot be read.
    pub fn resume(
        stream: OutputStream,
        reader: Box<dyn SpoolReader>,
        config: &SupervisorConfig,
    ) -> Result<Self, ProcessError> {
        let start = match reader.first_data_offset()? {
            Some(offset) => offset,
            None => reader.len()?,
        };
        let skip = if start > 0 { Skip::ToNewline } else { Skip::No };
        Ok(Self::at(stream, reader, config, start, skip, true))
    }

    fn at(
        stream: OutputStream,
        reader: Box<dyn SpoolReader>,
        config: &SupervisorConfig,
        start: u64,
        skip: Skip,
        resumed: bool,
    ) -> Self {
        Self {
            stream,
            reader,
            read_chunk: config.spool_read_chunk.max(1),
            line_max: config.log_line_max.max(4),
            hard_max: config.log_line_hard_max.max(1),
            backlog_max: config.spool_backlog_max,
            read_offset: start,
            held: Vec::new(),
            held_start: start,
            skip,
            continuing: false,
            line_end: start,
            accepted_end: start,
            pending: None,
            resumed,
            truncated: 0,
            release_failed: false,
            buf: Vec::new(),
        }
    }

    /// Returns the stream this reader reads.
    #[must_use]
    pub const fn stream(&self) -> OutputStream {
        self.stream
    }

    /// Returns the offset of the next byte to read.
    #[must_use]
    pub const fn read_offset(&self) -> u64 {
        self.read_offset
    }

    /// Returns where the last line the sink accepted ends, the offset the
    /// spool is freed up to (rounded down to a whole unit).
    #[must_use]
    pub const fn accepted_end(&self) -> u64 {
        self.accepted_end
    }

    /// Returns the size of the units the spool is freed in.
    #[must_use]
    pub fn punch_unit(&self) -> NonZeroU64 {
        self.reader.punch_unit()
    }

    /// Does one tick: offers a batch that waited, drops the oldest bytes
    /// when more than the backlog limit is unread, reads at most one chunk,
    /// and offers its lines to the sink.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when reading the spool fails. A failure
    /// to free bytes is logged once and does not stop the reading.
    pub fn pump(&mut self, ctx: &DrainContext<'_>) -> Result<DrainOutcome, ProcessError> {
        if self.flush_pending(ctx) == SinkOutcome::Busy {
            return Ok(DrainOutcome::Busy);
        }
        self.enforce_backlog()?;
        let full = self.read_one()?;
        let mut lines = Vec::new();
        self.extract_lines(ctx.redactor, &mut lines);
        if self.deliver(lines, ctx) == SinkOutcome::Busy {
            return Ok(DrainOutcome::Busy);
        }
        Ok(if full {
            DrainOutcome::More
        } else {
            DrainOutcome::CaughtUp
        })
    }

    /// Reads the spool to its end after the process exited, hands over a
    /// line still without its newline as a `partial` line, and frees what
    /// the sink accepted. When the sink is busy it returns
    /// [`DrainOutcome::Busy`] and the rest stays in the spool; calling it
    /// again carries on.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Io`] when reading the spool fails.
    pub fn finish(&mut self, ctx: &DrainContext<'_>) -> Result<DrainOutcome, ProcessError> {
        loop {
            match self.pump(ctx)? {
                DrainOutcome::More => {}
                DrainOutcome::Busy => return Ok(DrainOutcome::Busy),
                DrainOutcome::CaughtUp => break,
            }
        }
        if !self.held.is_empty() {
            let held = std::mem::take(&mut self.held);
            let mut lines = Vec::new();
            self.emit(ctx.redactor, &held, true, &mut lines);
            self.held_start = self.read_offset;
            self.line_end = self.read_offset;
            self.continuing = false;
            if self.deliver(lines, ctx) == SinkOutcome::Busy {
                return Ok(DrainOutcome::Busy);
            }
        }
        Ok(DrainOutcome::CaughtUp)
    }

    fn flush_pending(&mut self, ctx: &DrainContext<'_>) -> SinkOutcome {
        let Some(pending) = self.pending.take() else {
            return SinkOutcome::Accepted;
        };
        match ctx.sink.accept(ctx.instance, pending.batch.clone()) {
            SinkOutcome::Accepted => {
                self.accepted(pending.end);
                SinkOutcome::Accepted
            }
            SinkOutcome::Busy => {
                self.pending = Some(pending);
                SinkOutcome::Busy
            }
        }
    }

    /// Frees the unread bytes over the backlog limit, oldest first, and
    /// remembers how many for the next batch.
    fn enforce_backlog(&mut self) -> Result<(), ProcessError> {
        let len = self.reader.len()?;
        if len.saturating_sub(self.read_offset) <= self.backlog_max {
            return Ok(());
        }
        let keep_from = len - self.backlog_max;
        let held = u64::try_from(self.held.len()).unwrap_or(u64::MAX);
        self.truncated = self
            .truncated
            .saturating_add(keep_from - self.read_offset)
            .saturating_add(held);
        self.held.clear();
        self.read_offset = keep_from;
        self.held_start = keep_from;
        self.line_end = keep_from;
        self.skip = Skip::ToNewlineTruncated;
        self.continuing = false;
        self.free_below(keep_from);
        self.accepted_end = keep_from;
        Ok(())
    }

    /// Reads one chunk into `held` (or skips it). Returns whether the
    /// chunk was full.
    fn read_one(&mut self) -> Result<bool, ProcessError> {
        let mut buf = std::mem::take(&mut self.buf);
        buf.resize(self.read_chunk, 0);
        let result = self.reader.read_at(self.read_offset, &mut buf);
        let n = match result {
            Ok(n) => n,
            Err(error) => {
                self.buf = buf;
                return Err(error);
            }
        };
        let mut data = &buf[..n];
        self.read_offset += u64::try_from(n).unwrap_or(0);
        if self.skip != Skip::No {
            let (cut, found) = match data.iter().position(|byte| *byte == b'\n') {
                Some(at) => (at + 1, true),
                None => (data.len(), false),
            };
            if self.skip == Skip::ToNewlineTruncated {
                self.truncated = self
                    .truncated
                    .saturating_add(u64::try_from(cut).unwrap_or(0));
            }
            if found {
                self.skip = Skip::No;
            }
            data = &data[cut..];
            self.held_start = self.read_offset - u64::try_from(data.len()).unwrap_or(0);
            self.line_end = self.held_start;
        }
        self.held.extend_from_slice(data);
        let full = n == self.read_chunk;
        self.buf = buf;
        Ok(full)
    }

    /// Cuts whole lines (and over long pieces) off the front of `held`,
    /// redacts them, and appends them to `out`.
    fn extract_lines(&mut self, redactor: &Redactor, out: &mut Vec<LogLine>) {
        let held = std::mem::take(&mut self.held);
        let mut start = 0;
        loop {
            let rest = &held[start..];
            let newline = rest.iter().position(|byte| *byte == b'\n');
            match newline {
                Some(at) if at <= self.hard_max => {
                    let line = rest[..at].strip_suffix(b"\r").unwrap_or(&rest[..at]);
                    self.emit(redactor, line, false, out);
                    start += at + 1;
                    self.continuing = false;
                }
                _ if rest.len() > self.hard_max => {
                    let cut = utf8_cut(rest, self.hard_max);
                    self.emit(redactor, &rest[..cut], true, out);
                    start += cut;
                    self.continuing = true;
                }
                _ => break,
            }
            self.line_end = self.held_start + u64::try_from(start).unwrap_or(0);
        }
        self.held_start += u64::try_from(start).unwrap_or(0);
        self.held = held[start..].to_vec();
    }

    /// Redacts `bytes` as one logical line (or the first part of one),
    /// then cuts the result into pieces of at most `line_max` bytes.
    fn emit(&self, redactor: &Redactor, bytes: &[u8], partial: bool, out: &mut Vec<LogLine>) {
        let lossy = String::from_utf8_lossy(bytes);
        let redacted = redactor.redact(&lossy);
        let mut rest: &str = &redacted;
        let mut continued = self.continuing;
        loop {
            let (piece, tail) = rest.split_at(char_cut(rest, self.line_max));
            out.push(LogLine {
                text: piece.to_owned(),
                partial: partial && tail.is_empty(),
                continued,
            });
            if tail.is_empty() {
                break;
            }
            continued = true;
            rest = tail;
        }
    }

    /// Offers `lines` with the flags the reader owes the sink. A busy sink
    /// keeps the batch waiting in the reader.
    fn deliver(&mut self, lines: Vec<LogLine>, ctx: &DrainContext<'_>) -> SinkOutcome {
        if lines.is_empty() && self.truncated == 0 {
            return SinkOutcome::Accepted;
        }
        let mut batch = LogBatch::new(self.stream, ctx.clock.now(), lines);
        batch.resumed = std::mem::take(&mut self.resumed);
        batch.truncated = Some(std::mem::take(&mut self.truncated)).filter(|bytes| *bytes > 0);
        let end = self.line_end;
        match ctx.sink.accept(ctx.instance, batch.clone()) {
            SinkOutcome::Accepted => {
                self.accepted(end);
                SinkOutcome::Accepted
            }
            SinkOutcome::Busy => {
                self.pending = Some(Pending { batch, end });
                SinkOutcome::Busy
            }
        }
    }

    fn accepted(&mut self, end: u64) {
        self.accepted_end = self.accepted_end.max(end);
        self.free_below(end);
    }

    /// Frees the whole units below `offset`. A failure is logged once per
    /// reader and never stops the reading.
    fn free_below(&mut self, offset: u64) {
        if let Err(error) = self.reader.release_below(offset)
            && !self.release_failed
        {
            self.release_failed = true;
            tracing::warn!(
                stream = self.stream.as_code(),
                %error,
                "freeing the spool failed; reading goes on"
            );
        }
    }
}

impl std::fmt::Debug for SpoolDrain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpoolDrain")
            .field("stream", &self.stream)
            .field("read_offset", &self.read_offset)
            .field("accepted_end", &self.accepted_end)
            .finish_non_exhaustive()
    }
}

/// Returns where to cut `bytes` at `max`, stepping back so a UTF-8
/// sequence is not split (bytes that are not valid UTF-8 are cut at `max`).
fn utf8_cut(bytes: &[u8], max: usize) -> usize {
    let mut cut = max.min(bytes.len());
    let mut back = 0;
    while cut > 0 && back < 3 && bytes.get(cut).is_some_and(|byte| byte & 0xC0 == 0x80) {
        cut -= 1;
        back += 1;
    }
    if bytes.get(cut).is_some_and(|byte| byte & 0xC0 == 0x80) || cut == 0 {
        max.min(bytes.len())
    } else {
        cut
    }
}

/// Returns where to cut `text` at `max` bytes on a character boundary.
pub(super) fn char_cut(text: &str, max: usize) -> usize {
    if text.len() <= max {
        return text.len();
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_does_not_split_a_multibyte_character() {
        let text = "aé"; // 61 c3 a9
        assert_eq!(char_cut(text, 2), 1);
        assert_eq!(char_cut(text, 3), 3);
        assert_eq!(utf8_cut(text.as_bytes(), 2), 1);
        assert_eq!(utf8_cut(text.as_bytes(), 3), 3);
    }

    #[test]
    fn a_cut_of_bytes_that_are_not_utf8_falls_back_to_the_limit() {
        let bytes = [0x80_u8; 8];

        assert_eq!(utf8_cut(&bytes, 4), 4);
    }
}
