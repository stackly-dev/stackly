//! Segment files on disk: their names, the listing of a store folder, and
//! line readers that go forward or backward through one file.
//!
//! A segment is `<stream>.<firstSeq, 20 digits>.jsonl`. A file whose name
//! does not match is ignored and never deleted. Only lines that end in
//! `\n` exist: the tail of the active segment without its newline is a
//! write in progress (or one a crash cut), never a record.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use stackly_platform::{FileKind, Filesystem, OutputStream, PlatformError};

use super::record::peek_seq;

/// How much one read of a segment takes.
pub(crate) const CHUNK: usize = 64 * 1024;

/// One segment file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    /// The `seq` of its first record, from its name.
    pub(crate) first_seq: u64,
    /// Where it is.
    pub(crate) path: PathBuf,
    /// Its size when it was listed.
    pub(crate) len: u64,
}

/// Returns the file name of the segment of `stream` that starts at
/// `first_seq`.
pub(crate) fn segment_file_name(stream: OutputStream, first_seq: u64) -> String {
    format!("{}.{first_seq:020}.jsonl", stream.as_code())
}

fn parse_segment_name(name: &str) -> Option<(OutputStream, u64)> {
    let rest = name.strip_suffix(".jsonl")?;
    let (stream, digits) = rest.split_once('.')?;
    let stream = OutputStream::ALL
        .into_iter()
        .find(|candidate| candidate.as_code() == stream)?;
    if digits.len() != 20 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((stream, digits.parse().ok()?))
}

/// Returns the index of `stream` in a per stream array.
pub(crate) const fn slot(stream: OutputStream) -> usize {
    match stream {
        OutputStream::Stdout => 0,
        OutputStream::Stderr => 1,
    }
}

/// Lists the segments of both streams in `dir`, each oldest first. Only
/// regular files with a segment name count.
pub(crate) fn list_segments(
    fs: &dyn Filesystem,
    dir: &Path,
) -> Result<[Vec<Segment>; 2], PlatformError> {
    let mut out: [Vec<Segment>; 2] = [Vec::new(), Vec::new()];
    for entry in fs.read_dir(dir)? {
        if entry.info.kind != FileKind::File {
            continue;
        }
        let Some((stream, first_seq)) = entry.name.to_str().and_then(parse_segment_name) else {
            continue;
        };
        out[slot(stream)].push(Segment {
            first_seq,
            path: dir.join(&entry.name),
            len: entry.info.len,
        });
    }
    for segments in &mut out {
        segments.sort_by_key(|segment| segment.first_seq);
    }
    Ok(out)
}

/// One complete line of a segment, without its `\n`.
#[derive(Debug)]
pub(crate) struct Line {
    /// Where the line starts.
    pub(crate) start: u64,
    /// The bytes of the line.
    pub(crate) bytes: Vec<u8>,
}

impl Line {
    /// Where the line ends, after its `\n`.
    pub(crate) fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64 + 1
    }
}

/// Reads the complete lines of a file forward from an offset.
pub(crate) struct Forward<R> {
    file: R,
    /// The next byte to read from the file.
    pos: u64,
    /// The end of the file as it was opened.
    end: u64,
    buf: Vec<u8>,
    /// The offset of `buf[0]`.
    buf_start: u64,
    /// Bytes read from the file so far.
    pub(crate) scanned: u64,
}

impl<R: Read + Seek> Forward<R> {
    /// Starts at `from`, which must be the start of a line, and stops at
    /// `end`.
    pub(crate) const fn new(file: R, from: u64, end: u64) -> Self {
        Self {
            file,
            pos: from,
            end,
            buf: Vec::new(),
            buf_start: from,
            scanned: 0,
        }
    }

    /// Returns the next complete line, or `None` at the end (a last line
    /// without its `\n` is left unread).
    pub(crate) fn next_line(&mut self) -> io::Result<Option<Line>> {
        loop {
            if let Some(at) = self.buf.iter().position(|byte| *byte == b'\n') {
                let bytes = self.buf[..at].to_vec();
                self.buf.drain(..=at);
                let start = self.buf_start;
                self.buf_start += at as u64 + 1;
                return Ok(Some(Line { start, bytes }));
            }
            if self.pos >= self.end {
                return Ok(None);
            }
            let want = usize::try_from(self.end - self.pos)
                .unwrap_or(usize::MAX)
                .min(CHUNK);
            let mut chunk = vec![0; want];
            self.file.seek(SeekFrom::Start(self.pos))?;
            let n = self.file.read(&mut chunk)?;
            if n == 0 {
                self.end = self.pos;
                return Ok(None);
            }
            self.pos += n as u64;
            self.scanned += n as u64;
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
}

/// Reads the complete lines of a file backward from an offset.
pub(crate) struct Backward<R> {
    file: R,
    /// The first byte not read yet, counting down.
    pos: u64,
    /// Bytes read that do not start a line yet; they end at `buf_end`.
    buf: Vec<u8>,
    buf_end: u64,
    started: bool,
    /// Bytes read from the file so far.
    pub(crate) scanned: u64,
}

impl<R: Read + Seek> Backward<R> {
    /// Starts at `until`, which is either the end of the file or the start
    /// of a line. Bytes after the last `\n` before `until` are not a line.
    pub(crate) const fn new(file: R, until: u64) -> Self {
        Self {
            file,
            pos: until,
            buf: Vec::new(),
            buf_end: until,
            started: false,
            scanned: 0,
        }
    }

    /// Returns the line before the last one returned, or `None` at the
    /// start of the file.
    pub(crate) fn next_line(&mut self) -> io::Result<Option<Line>> {
        loop {
            if !self.started {
                // Drop what follows the last newline: a write in progress.
                if let Some(at) = self.buf.iter().rposition(|byte| *byte == b'\n') {
                    self.buf.truncate(at + 1);
                    self.buf_end = self.pos + at as u64 + 1;
                    self.started = true;
                } else if self.pos == 0 {
                    return Ok(None);
                }
            }
            if self.started {
                // Only the line at offset 0 leaves the buffer empty.
                if self.buf.is_empty() {
                    return Ok(None);
                }
                // `buf` ends with the newline of the line to return.
                let body = &self.buf[..self.buf.len() - 1];
                if let Some(at) = body.iter().rposition(|byte| *byte == b'\n') {
                    let bytes = body[at + 1..].to_vec();
                    let start = self.pos + at as u64 + 1;
                    self.buf.truncate(at + 1);
                    self.buf_end = start;
                    return Ok(Some(Line { start, bytes }));
                }
                if self.pos == 0 {
                    if self.buf.is_empty() {
                        return Ok(None);
                    }
                    let bytes = body.to_vec();
                    self.buf.clear();
                    self.buf_end = 0;
                    return Ok(Some(Line { start: 0, bytes }));
                }
            }
            let take = usize::try_from(self.pos).unwrap_or(usize::MAX).min(CHUNK);
            let from = self.pos - take as u64;
            let mut chunk = vec![0; take];
            self.file.seek(SeekFrom::Start(from))?;
            self.file.read_exact(&mut chunk)?;
            self.scanned += take as u64;
            chunk.extend_from_slice(&self.buf);
            self.buf = chunk;
            self.pos = from;
        }
    }
}

/// Returns the offset of the first complete line of the file whose `seq`
/// is above `seq` (the end when none is), by a binary search over line
/// starts and a short forward scan.
pub(crate) fn first_after<R: Read + Seek>(file: &mut R, len: u64, seq: u64) -> io::Result<u64> {
    let mut lo = 0;
    let mut hi = len;
    while hi - lo > CHUNK as u64 {
        let mid = lo + (hi - lo) / 2;
        let Some((start, end, found)) = line_from(file, mid, hi)? else {
            break;
        };
        match found {
            Some(found) if found <= seq => lo = end,
            Some(_) => hi = start,
            None => break,
        }
    }
    let mut forward = Forward::new(&mut *file, lo, hi);
    while let Some(line) = forward.next_line()? {
        if peek_seq(&line.bytes).is_some_and(|found| found > seq) {
            return Ok(line.start);
        }
    }
    Ok(hi)
}

/// Returns the first complete line that starts at or after `at` and before
/// `hi`: its start, its end, and its `seq` when it has one.
fn line_from<R: Read + Seek>(
    file: &mut R,
    at: u64,
    hi: u64,
) -> io::Result<Option<(u64, u64, Option<u64>)>> {
    let from = at.saturating_sub(1);
    let mut forward = Forward::new(&mut *file, from, hi);
    // The bytes up to the first newline belong to the line around `at`.
    if from > 0 && forward.next_line()?.is_none() {
        return Ok(None);
    }
    Ok(forward
        .next_line()?
        .map(|line| (line.start, line.end(), peek_seq(&line.bytes))))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn file_of(seqs: std::ops::RangeInclusive<u64>, pad: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for seq in seqs {
            out.extend_from_slice(
                format!("{{\"seq\":{seq},\"x\":\"{}\"}}\n", "a".repeat(pad)).as_bytes(),
            );
        }
        out
    }

    #[test]
    fn segment_names_round_trip_and_others_are_ignored() {
        let name = segment_file_name(OutputStream::Stderr, 42);
        assert_eq!(name, "stderr.00000000000000000042.jsonl");
        assert_eq!(parse_segment_name(&name), Some((OutputStream::Stderr, 42)));
        assert_eq!(parse_segment_name("stdout.42.jsonl"), None);
        assert_eq!(parse_segment_name("store.json"), None);
        assert_eq!(parse_segment_name("notes.00000000000000000001.jsonl"), None);
    }

    #[test]
    fn forward_skips_a_last_line_without_its_newline() {
        let mut bytes = file_of(1..=3, 4);
        bytes.extend_from_slice(b"{\"seq\":4,");
        let len = bytes.len() as u64;
        let mut forward = Forward::new(Cursor::new(bytes), 0, len);

        let mut seqs = Vec::new();
        while let Some(line) = forward.next_line().unwrap() {
            seqs.push(peek_seq(&line.bytes).unwrap());
        }

        assert_eq!(seqs, [1, 2, 3]);
    }

    #[test]
    fn backward_returns_lines_newest_first_across_chunks() {
        let mut bytes = file_of(1..=3000, 50);
        bytes.extend_from_slice(b"{\"seq\":3001");
        let len = bytes.len() as u64;
        let mut backward = Backward::new(Cursor::new(bytes), len);

        let mut seqs = Vec::new();
        while let Some(line) = backward.next_line().unwrap() {
            seqs.push(peek_seq(&line.bytes).unwrap());
        }

        assert_eq!(seqs, (1..=3000).rev().collect::<Vec<_>>());
    }

    #[test]
    fn backward_from_a_line_start_returns_the_lines_before_it() {
        let bytes = file_of(1..=5, 1);
        let line_len = bytes.len() as u64 / 5;
        let mut backward = Backward::new(Cursor::new(bytes), line_len * 3);

        let first = backward.next_line().unwrap().unwrap();

        assert_eq!(peek_seq(&first.bytes), Some(3));
        assert_eq!(first.end(), line_len * 3);
    }

    #[test]
    fn the_binary_search_finds_the_first_line_after_a_seq() {
        let bytes = file_of(10..=5000, 40);
        let len = bytes.len() as u64;
        let mut file = Cursor::new(bytes);
        for target in [0, 9, 10, 11, 2500, 4999, 5000, 9000] {
            let offset = first_after(&mut file, len, target).unwrap();
            let mut forward = Forward::new(&mut file, offset, len);
            let next = forward
                .next_line()
                .unwrap()
                .map(|line| peek_seq(&line.bytes).unwrap());

            let expected = if target >= 5000 {
                None
            } else {
                Some(target.max(9) + 1)
            };
            assert_eq!(next, expected, "after {target}");
        }
    }
}
