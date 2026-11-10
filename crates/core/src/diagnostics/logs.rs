//! The two logs of a bundle (spec 0034, *Isi bundle*): the tail of the
//! daemon log across its rotated files, and the newest entries of one
//! instance from the durable log store (spec 0027), one line per entry.
//! Both only read.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use jiff::Timestamp;
use stackly_platform::{Filesystem, OutputStream, PlatformError};

use super::bundle::{
    DAEMON_LOG_BYTES_MAX, DAEMON_LOG_LINES_MAX, INSTANCE_LOG_BYTES_MAX, INSTANCE_LOG_ENTRIES_MAX,
    LogText,
};
use crate::logs::{
    LOG_SCAN_BUDGET, LogCursor, LogEntry, LogFilter, LogPosition, LogReadError, ReadRequest,
    RecordKind, read_page,
};

/// The daemon log files, newest first (spec 0006 AC-12).
const DAEMON_LOG_FILES: [&str; 5] = [
    "stacklyd.log",
    "stacklyd.log.1",
    "stacklyd.log.2",
    "stacklyd.log.3",
    "stacklyd.log.4",
];

/// The last [`DAEMON_LOG_LINES_MAX`] lines of the daemon log in `dir`,
/// across `stacklyd.log` to `stacklyd.log.4`, at most
/// [`DAEMON_LOG_BYTES_MAX`] bytes. A file that is not there is skipped.
///
/// # Errors
///
/// Returns the error of a file that exists but cannot be read.
pub fn daemon_log(fs: &dyn Filesystem, dir: &Path) -> Result<LogText, PlatformError> {
    let budget = u64::try_from(DAEMON_LOG_BYTES_MAX).unwrap_or(u64::MAX);
    let mut lines: Vec<String> = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    for (index, name) in DAEMON_LOG_FILES.iter().enumerate() {
        if lines.len() >= DAEMON_LOG_LINES_MAX || bytes >= DAEMON_LOG_BYTES_MAX {
            truncated = truncated
                || DAEMON_LOG_FILES[index..].iter().any(|name| {
                    fs.symlink_metadata(&dir.join(name))
                        .ok()
                        .flatten()
                        .is_some()
                });
            break;
        }
        let mut file = match fs.open_read(&dir.join(name)) {
            Ok(file) => file,
            Err(PlatformError::NotFound { .. }) => continue,
            Err(error) => return Err(error),
        };
        let len = file.len();
        let start = len.saturating_sub(budget);
        let io = |error: std::io::Error| {
            PlatformError::from_io("read daemon log", &dir.join(name), &error)
        };
        file.seek(SeekFrom::Start(start)).map_err(io)?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw).map_err(io)?;
        let mut text = String::from_utf8_lossy(&raw).into_owned();
        if start > 0 {
            // The first line was cut by the seek.
            truncated = true;
            text = text
                .split_once('\n')
                .map(|(_, rest)| rest.to_owned())
                .unwrap_or_default();
        }
        let mut older: Vec<String> = text.lines().map(str::to_owned).collect();
        bytes += older.iter().map(|line| line.len() + 1).sum::<usize>();
        older.append(&mut lines);
        lines = older;
    }
    if lines.len() > DAEMON_LOG_LINES_MAX {
        truncated = true;
        lines.drain(..lines.len() - DAEMON_LOG_LINES_MAX);
    }
    let mut size: usize = lines.iter().map(|line| line.len() + 1).sum();
    let mut cut = 0;
    while size > DAEMON_LOG_BYTES_MAX && cut < lines.len() {
        size -= lines[cut].len() + 1;
        cut += 1;
        truncated = true;
    }
    lines.drain(..cut);
    Ok(LogText::new(lines, truncated))
}

/// The newest [`INSTANCE_LOG_ENTRIES_MAX`] entries of the store in `dir`,
/// every stream, oldest first, one line `<RFC 3339> <stream> <text>` each,
/// at most [`INSTANCE_LOG_BYTES_MAX`] bytes, and the time of the newest
/// entry. `segments_per_stream` is the store's rotation setting.
///
/// # Errors
///
/// Returns [`LogReadError`] when the store cannot be read.
pub fn instance_log(
    fs: &dyn Filesystem,
    dir: &Path,
    segments_per_stream: usize,
) -> Result<(LogText, Option<Timestamp>), LogReadError> {
    let mut entries: Vec<LogEntry> = Vec::new();
    let mut position = LogPosition::Newest;
    let mut bytes = 0usize;
    let more = loop {
        let request = ReadRequest {
            filter: LogFilter::all(),
            position,
            limit: INSTANCE_LOG_ENTRIES_MAX - entries.len(),
            byte_limit: INSTANCE_LOG_BYTES_MAX * 2,
            scan_budget: LOG_SCAN_BUDGET,
            segments_per_stream,
        };
        let page = read_page(fs, dir, &request)?;
        let read = page.entries.len();
        bytes += page
            .entries
            .iter()
            .map(|entry| line(entry).len() + 1)
            .sum::<usize>();
        let mut older = page.entries;
        older.append(&mut entries);
        entries = older;
        match (page.earlier, page.store) {
            (Some(seq), Some(store))
                if read > 0
                    && entries.len() < INSTANCE_LOG_ENTRIES_MAX
                    && bytes < INSTANCE_LOG_BYTES_MAX =>
            {
                position = LogPosition::Before(LogCursor { store, seq });
            }
            (earlier, _) => break earlier.is_some(),
        }
    };
    let newest = entries.last().map(|entry| entry.at);
    let mut lines: Vec<String> = entries.iter().map(line).collect();
    let mut size: usize = lines.iter().map(|line| line.len() + 1).sum();
    let mut cut = 0;
    while size > INSTANCE_LOG_BYTES_MAX && cut < lines.len() {
        size -= lines[cut].len() + 1;
        cut += 1;
    }
    lines.drain(..cut);
    Ok((LogText::new(lines, more || cut > 0), newest))
}

/// One entry as a line of the bundle.
fn line(entry: &LogEntry) -> String {
    let stream = match entry.stream {
        Some(OutputStream::Stdout) => "stdout",
        Some(OutputStream::Stderr) => "stderr",
        None => "stackly",
    };
    let text = match &entry.kind {
        RecordKind::Line(line) => line.text.clone(),
        RecordKind::Truncated { bytes } => format!("[{bytes} bytes of output were dropped]"),
        RecordKind::Started { pid } => format!("[the process started with PID {pid}]"),
        RecordKind::Exited { code, signal } => match (code, signal) {
            (Some(code), _) => format!("[the process exited with code {code}]"),
            (None, Some(signal)) => format!("[the process ended by signal {signal}]"),
            (None, None) => "[the process exited]".to_owned(),
        },
        RecordKind::Unavailable => {
            "[output written while no daemon read it is unavailable]".to_owned()
        }
    };
    format!("{} {stream} {text}", entry.at)
}
