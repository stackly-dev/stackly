//! Pages of the durable log store (spec 0027, *Pembaca*, *Merge dua
//! stream*, *Aturan halaman*): the newest records, or the records right
//! before or after a cursor, read straight from the segments.
//!
//! Two cursors, one per stream, run side by side and the record with the
//! smallest `seq` (the largest, going backward) is taken; a run marker
//! written to both files comes out once. A read stops at its `limit`, at
//! its byte limit (the first entry always fits), or after scanning its
//! budget, and the cursor it returns is the frontier closest to where it
//! began, so nothing is skipped.

use std::path::Path;

use jiff::Timestamp;
use stackly_platform::{Filesystem, OutputStream, PlatformError, ReadSeekFile};

use super::filter::LogFilter;
use super::record::{Record, RecordKind};
use super::scan::{Backward, Forward, Segment, first_after, list_segments, slot};
use super::store::{Folder, LogCursor, LogStoreId, StoreFailure, check_folder, read_store_id};

/// Where a page starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogPosition {
    /// The newest records.
    Newest,
    /// The records with a `seq` below the cursor, the closest ones.
    Before(LogCursor),
    /// The records with a `seq` above the cursor, the closest ones.
    After(LogCursor),
    /// The oldest records of the store.
    Start,
}

/// A read.
#[derive(Debug, Clone)]
pub struct ReadRequest {
    /// What is kept.
    pub filter: LogFilter,
    /// Where it starts.
    pub position: LogPosition,
    /// The entries at most.
    pub limit: usize,
    /// The encoded bytes of the entries at most; the first always fits.
    pub byte_limit: usize,
    /// The bytes scanned at most, across both streams.
    pub scan_budget: u64,
    /// The segments a stream keeps, to tell a rotated cursor.
    pub segments_per_stream: usize,
}

/// Why the records right after a cursor are not the next ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogGap {
    /// Records at the cursor were rotated out.
    Rotated,
    /// The cursor belongs to another store.
    StoreReset,
}

/// One entry of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Its `seq`.
    pub seq: u64,
    /// When the daemon read it.
    pub at: Timestamp,
    /// Its stream; `None` for a run marker.
    pub stream: Option<OutputStream>,
    /// What it holds.
    pub kind: RecordKind,
}

/// One page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogPage {
    /// The store; `None` when it does not exist yet.
    pub store: Option<LogStoreId>,
    /// The entries, `seq` rising.
    pub entries: Vec<LogEntry>,
    /// The cursor for the page before; `None` when nothing older is left.
    pub earlier: Option<u64>,
    /// The cursor for the page after; `None` when the page reached the
    /// newest record.
    pub later: Option<u64>,
    /// The newest `seq` of the store.
    pub head: Option<u64>,
    /// Applies before the entries.
    pub gap: Option<LogGap>,
    /// The read stopped at its scan budget.
    pub scan_limited: bool,
    /// The bytes of the segments of stdout and stderr.
    pub stored_bytes: [u64; 2],
    /// Whole lines that were not records, met by this read.
    pub skipped_records: u64,
    /// The largest `seq` this read looked at going forward, kept or not.
    pub examined: Option<u64>,
}

/// A read that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LogReadError {
    /// The store folder or a file of it cannot be read.
    #[error("the log store cannot be read ({})", .0.as_code())]
    Unreadable(StoreFailure),
}

fn unreadable(error: &PlatformError) -> LogReadError {
    LogReadError::Unreadable(StoreFailure::of(error))
}

/// The cost of an entry against the byte limit: its record encoded, plus
/// room for the wider keys of the wire shape.
fn cost(record: &Record) -> usize {
    record.encode().len() + 64
}

/// Reads one page of the store in `dir`.
///
/// # Errors
///
/// Returns [`LogReadError::Unreadable`] when the folder, the header, or a
/// segment cannot be read. A segment that is gone (rotated or deleted) is
/// never an error.
pub fn read_page(
    fs: &dyn Filesystem,
    dir: &Path,
    request: &ReadRequest,
) -> Result<LogPage, LogReadError> {
    if let Folder::Missing = check_folder(fs, dir).map_err(|error| unreadable(&error))? {
        return Ok(LogPage::default());
    }
    let segments = match list_segments(fs, dir) {
        Ok(segments) => segments,
        Err(PlatformError::NotFound { .. }) => return Ok(LogPage::default()),
        Err(error) => return Err(unreadable(&error)),
    };
    let store = read_store_id(fs, dir).map_err(|error| unreadable(&error))?;
    let mut page = LogPage {
        store,
        stored_bytes: [0, 1].map(|index| segments[index].iter().map(|segment| segment.len).sum()),
        ..LogPage::default()
    };
    for stream in OutputStream::ALL {
        let seq = last_seq(fs, &segments[slot(stream)])?;
        page.head = page.head.max(seq);
    }

    let mut position = request.position;
    let foreign = |cursor: &LogCursor| store.is_some_and(|store| store != cursor.store);
    match position {
        LogPosition::Before(cursor) if foreign(&cursor) => {
            page.gap = Some(LogGap::StoreReset);
            position = LogPosition::Newest;
        }
        LogPosition::After(cursor) if foreign(&cursor) => {
            page.gap = Some(LogGap::StoreReset);
            position = LogPosition::Start;
        }
        _ => {}
    }
    let streams = request.filter.streams.streams();
    let mut oldest = None;
    let mut rotated_from = None;
    for stream in &streams {
        let list = &segments[slot(*stream)];
        let Some(first) = list.first() else { continue };
        let first_seq = first_seq(fs, first)?.unwrap_or(first.first_seq);
        oldest = Some(oldest.map_or(first_seq, |seen: u64| seen.min(first_seq)));
        if list.len() >= request.segments_per_stream {
            rotated_from = Some(rotated_from.map_or(first_seq, |seen: u64| seen.max(first_seq)));
        }
    }
    match position {
        LogPosition::After(cursor) => {
            if rotated_from.is_some_and(|from| from > cursor.seq + 1) {
                page.gap = Some(LogGap::Rotated);
            }
            read_forward(fs, &segments, request, Some(cursor.seq), oldest, &mut page)?;
        }
        LogPosition::Start => read_forward(fs, &segments, request, None, oldest, &mut page)?,
        LogPosition::Before(cursor) => {
            if rotated_from.is_some() && oldest.is_some_and(|oldest| oldest >= cursor.seq) {
                page.gap = Some(LogGap::Rotated);
                return Ok(page);
            }
            read_backward(fs, &segments, request, Some(cursor.seq), &mut page)?;
        }
        LogPosition::Newest => read_backward(fs, &segments, request, None, &mut page)?,
    }
    Ok(page)
}

/// Returns the `seq` of the newest whole record of one stream.
fn last_seq(fs: &dyn Filesystem, segments: &[Segment]) -> Result<Option<u64>, LogReadError> {
    for segment in segments.iter().rev().take(2) {
        let Some(mut file) = open(fs, segment)? else {
            continue;
        };
        let len = file.len();
        let mut backward = Backward::new(&mut file, len);
        while let Some(line) = backward
            .next_line()
            .map_err(|error| io_error(segment, &error))?
        {
            if let Some(record) = Record::decode(&line.bytes) {
                return Ok(Some(record.seq));
            }
        }
    }
    Ok(None)
}

/// Returns the `seq` of the first whole record of a segment.
fn first_seq(fs: &dyn Filesystem, segment: &Segment) -> Result<Option<u64>, LogReadError> {
    let Some(mut file) = open(fs, segment)? else {
        return Ok(None);
    };
    let len = file.len();
    let mut forward = Forward::new(&mut file, 0, len);
    while let Some(line) = forward
        .next_line()
        .map_err(|error| io_error(segment, &error))?
    {
        if let Some(record) = Record::decode(&line.bytes) {
            return Ok(Some(record.seq));
        }
    }
    Ok(None)
}

/// Opens a segment; `None` when it is gone.
fn open(
    fs: &dyn Filesystem,
    segment: &Segment,
) -> Result<Option<Box<dyn ReadSeekFile>>, LogReadError> {
    match fs.open_read(&segment.path) {
        Ok(file) => Ok(Some(file)),
        Err(PlatformError::NotFound { .. }) => Ok(None),
        Err(error) => Err(unreadable(&error)),
    }
}

fn io_error(segment: &Segment, error: &std::io::Error) -> LogReadError {
    unreadable(&PlatformError::from_io("read_log", &segment.path, error))
}

/// One stream read forward.
struct Ahead<'a> {
    stream: OutputStream,
    segments: &'a [Segment],
    index: usize,
    reader: Option<Forward<Box<dyn ReadSeekFile>>>,
    peeked: Option<Record>,
    scanned: u64,
    lost: bool,
}

impl Ahead<'_> {
    /// Returns the next record, reading on as needed.
    fn peek(
        &mut self,
        fs: &dyn Filesystem,
        skipped: &mut u64,
    ) -> Result<Option<&Record>, LogReadError> {
        while self.peeked.is_none() {
            let Some(reader) = &mut self.reader else {
                self.index += 1;
                let Some(segment) = self.segments.get(self.index) else {
                    return Ok(None);
                };
                match open(fs, segment)? {
                    Some(file) => {
                        let len = file.len();
                        self.reader = Some(Forward::new(file, 0, len));
                    }
                    None => self.lost = true,
                }
                continue;
            };
            let before = reader.scanned;
            let line = reader
                .next_line()
                .map_err(|error| io_error(&self.segments[self.index], &error))?;
            self.scanned += reader.scanned - before;
            match line {
                Some(line) => match Record::decode(&line.bytes) {
                    Some(record) => self.peeked = Some(record),
                    None => *skipped += 1,
                },
                None => self.reader = None,
            }
        }
        Ok(self.peeked.as_ref())
    }
}

fn read_forward(
    fs: &dyn Filesystem,
    segments: &[Vec<Segment>; 2],
    request: &ReadRequest,
    after: Option<u64>,
    oldest: Option<u64>,
    page: &mut LogPage,
) -> Result<(), LogReadError> {
    let mut cursors = Vec::new();
    for stream in request.filter.streams.streams() {
        let list = &segments[slot(stream)];
        let mut ahead = Ahead {
            stream,
            segments: list,
            index: 0,
            reader: None,
            peeked: None,
            scanned: 0,
            lost: false,
        };
        if let Some(after) = after {
            // The newest segment that may hold `after + 1`.
            let index = list
                .iter()
                .rposition(|segment| segment.first_seq <= after.saturating_add(1))
                .unwrap_or(0);
            if let Some(segment) = list.get(index)
                && let Some(mut file) = open(fs, segment)?
            {
                let len = file.len();
                let offset = first_after(&mut file, len, after)
                    .map_err(|error| io_error(segment, &error))?;
                ahead.index = index;
                ahead.reader = Some(Forward::new(file, offset, len));
            } else {
                ahead.index = index;
            }
        } else if let Some(segment) = list.first()
            && let Some(file) = open(fs, segment)?
        {
            let len = file.len();
            ahead.reader = Some(Forward::new(file, 0, len));
        }
        cursors.push(ahead);
    }

    let mut bytes = 0;
    let mut consumed = 0_u64;
    let mut last = after;
    let mut stopped = false;
    loop {
        let mut best: Option<(usize, u64)> = None;
        for (index, cursor) in cursors.iter_mut().enumerate() {
            if let Some(record) = cursor.peek(fs, &mut page.skipped_records)?
                && best.is_none_or(|(_, seq)| record.seq < seq)
            {
                best = Some((index, record.seq));
            }
        }
        let Some((index, seq)) = best else { break };
        // At least one record is looked at, so every read moves on.
        let scanned: u64 = cursors.iter().map(|cursor| cursor.scanned).sum();
        if consumed > 0 && scanned >= request.scan_budget {
            page.scan_limited = true;
            stopped = true;
            break;
        }
        let record = cursors[index]
            .peeked
            .take()
            .expect("the record was peeked right above");
        consumed += 1;
        if last.is_some_and(|last| seq <= last) {
            continue;
        }
        if request.filter.admits(&record) {
            let size = cost(&record);
            if page.entries.len() >= request.limit
                || (!page.entries.is_empty() && bytes + size > request.byte_limit)
            {
                cursors[index].peeked = Some(record);
                stopped = true;
                break;
            }
            bytes += size;
            page.entries.push(entry(cursors[index].stream, record));
        }
        last = Some(seq);
    }
    if cursors.iter().any(|cursor| cursor.lost) {
        page.gap = page.gap.or(Some(LogGap::Rotated));
    }
    page.examined = last;
    page.later = if stopped { last.or(after) } else { None };
    page.earlier = page
        .entries
        .first()
        .map(|entry| entry.seq)
        .filter(|seq| oldest.is_some_and(|oldest| *seq > oldest));
    Ok(())
}

/// One stream read backward.
struct Behind<'a> {
    stream: OutputStream,
    segments: &'a [Segment],
    /// The segment being read; it counts down.
    index: Option<usize>,
    reader: Option<Backward<Box<dyn ReadSeekFile>>>,
    peeked: Option<Record>,
    scanned: u64,
}

impl Behind<'_> {
    fn peek(
        &mut self,
        fs: &dyn Filesystem,
        skipped: &mut u64,
    ) -> Result<Option<&Record>, LogReadError> {
        while self.peeked.is_none() {
            let Some(index) = self.index else {
                return Ok(None);
            };
            let Some(reader) = &mut self.reader else {
                match open(fs, &self.segments[index])? {
                    Some(file) => {
                        let len = file.len();
                        self.reader = Some(Backward::new(file, len));
                    }
                    // Older segments were rotated out.
                    None => self.index = None,
                }
                continue;
            };
            let before = reader.scanned;
            let line = reader
                .next_line()
                .map_err(|error| io_error(&self.segments[index], &error))?;
            self.scanned += reader.scanned - before;
            match line {
                Some(line) => match Record::decode(&line.bytes) {
                    Some(record) => self.peeked = Some(record),
                    None => *skipped += 1,
                },
                None => {
                    self.reader = None;
                    self.index = index.checked_sub(1);
                }
            }
        }
        Ok(self.peeked.as_ref())
    }
}

fn read_backward(
    fs: &dyn Filesystem,
    segments: &[Vec<Segment>; 2],
    request: &ReadRequest,
    before: Option<u64>,
    page: &mut LogPage,
) -> Result<(), LogReadError> {
    let mut cursors = Vec::new();
    for stream in request.filter.streams.streams() {
        let list = &segments[slot(stream)];
        let mut behind = Behind {
            stream,
            segments: list,
            index: list.len().checked_sub(1),
            reader: None,
            peeked: None,
            scanned: 0,
        };
        if let Some(before) = before {
            let index = list.iter().rposition(|segment| segment.first_seq < before);
            behind.index = index;
            if let Some(index) = index
                && let Some(mut file) = open(fs, &list[index])?
            {
                let len = file.len();
                let until = first_after(&mut file, len, before.saturating_sub(1))
                    .map_err(|error| io_error(&list[index], &error))?;
                behind.reader = Some(Backward::new(file, until));
            }
        }
        cursors.push(behind);
    }

    let mut bytes = 0;
    let mut consumed = 0_u64;
    let mut last = before;
    let mut highest = None;
    let mut stopped = false;
    loop {
        let mut best: Option<(usize, u64)> = None;
        for (index, cursor) in cursors.iter_mut().enumerate() {
            if let Some(record) = cursor.peek(fs, &mut page.skipped_records)?
                && best.is_none_or(|(_, seq)| record.seq > seq)
            {
                best = Some((index, record.seq));
            }
        }
        let Some((index, seq)) = best else { break };
        // At least one record is looked at, so every read moves on.
        let scanned: u64 = cursors.iter().map(|cursor| cursor.scanned).sum();
        if consumed > 0 && scanned >= request.scan_budget {
            page.scan_limited = true;
            stopped = true;
            break;
        }
        let record = cursors[index]
            .peeked
            .take()
            .expect("the record was peeked right above");
        consumed += 1;
        if last.is_some_and(|last| seq >= last) {
            continue;
        }
        if request.filter.admits(&record) {
            let size = cost(&record);
            if page.entries.len() >= request.limit
                || (!page.entries.is_empty() && bytes + size > request.byte_limit)
            {
                cursors[index].peeked = Some(record);
                stopped = true;
                break;
            }
            bytes += size;
            page.entries.push(entry(cursors[index].stream, record));
        }
        highest = highest.or(Some(seq));
        last = Some(seq);
    }
    page.entries.reverse();
    page.earlier = if stopped { last.or(before) } else { None };
    page.later = match before {
        None => None,
        Some(before) => {
            let top = highest.unwrap_or(before.saturating_sub(1));
            Some(top).filter(|top| page.head.is_none_or(|head| *top < head))
        }
    };
    Ok(())
}

fn entry(stream: OutputStream, record: Record) -> LogEntry {
    LogEntry {
        seq: record.seq,
        at: record.at,
        stream: (!record.kind.is_run_marker()).then_some(stream),
        kind: record.kind,
    }
}
