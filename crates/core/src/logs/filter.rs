//! Which records a read keeps (spec 0027, *Aturan halaman*, Filter).
//!
//! The stream filter picks the segment files read at all, so it hides line
//! and `truncated` records of the other stream; run markers sit in both
//! files and always pass it. The text filter keeps lines whose text holds
//! it in any case; `truncated` and `unavailable` always pass so a loss
//! stays visible, and `started` and `exited` show only without it.

use stackly_platform::OutputStream;

use super::record::{Record, RecordKind};

/// The streams a read covers. Never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamSet {
    stdout: bool,
    stderr: bool,
}

impl StreamSet {
    /// Both streams.
    pub const ALL: Self = Self {
        stdout: true,
        stderr: true,
    };

    /// Only `stream`.
    #[must_use]
    pub const fn only(stream: OutputStream) -> Self {
        match stream {
            OutputStream::Stdout => Self {
                stdout: true,
                stderr: false,
            },
            OutputStream::Stderr => Self {
                stdout: false,
                stderr: true,
            },
        }
    }

    /// The set of `streams`; an empty list means both.
    #[must_use]
    pub fn of(streams: &[OutputStream]) -> Self {
        if streams.is_empty() {
            return Self::ALL;
        }
        Self {
            stdout: streams.contains(&OutputStream::Stdout),
            stderr: streams.contains(&OutputStream::Stderr),
        }
    }

    /// Returns whether the set covers `stream`.
    #[must_use]
    pub const fn contains(self, stream: OutputStream) -> bool {
        match stream {
            OutputStream::Stdout => self.stdout,
            OutputStream::Stderr => self.stderr,
        }
    }

    /// The streams of the set, stdout first.
    #[must_use]
    pub fn streams(self) -> Vec<OutputStream> {
        OutputStream::ALL
            .into_iter()
            .filter(|stream| self.contains(*stream))
            .collect()
    }
}

/// What a read keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFilter {
    /// The streams read.
    pub streams: StreamSet,
    contains: Option<String>,
}

impl LogFilter {
    /// Keeps everything of both streams.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            streams: StreamSet::ALL,
            contains: None,
        }
    }

    /// Keeps records of `streams` and, when `contains` is given, lines that
    /// hold it in any case.
    #[must_use]
    pub fn new(streams: StreamSet, contains: Option<&str>) -> Self {
        Self {
            streams,
            contains: contains.map(str::to_lowercase),
        }
    }

    /// Returns whether a record read from one of the filter's streams is
    /// kept.
    #[must_use]
    pub fn admits(&self, record: &Record) -> bool {
        let Some(needle) = &self.contains else {
            return true;
        };
        match &record.kind {
            RecordKind::Line(line) => line.text.to_lowercase().contains(needle.as_str()),
            RecordKind::Truncated { .. } | RecordKind::Unavailable => true,
            RecordKind::Started { .. } | RecordKind::Exited { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::*;
    use crate::logs::level::LogLevel;
    use crate::logs::record::LineRecord;

    fn line(text: &str) -> Record {
        Record::new(
            1,
            Timestamp::UNIX_EPOCH,
            RecordKind::Line(LineRecord {
                text: text.to_owned(),
                level: LogLevel::Normal,
                partial: false,
                continued: false,
                step: None,
                src: None,
            }),
        )
    }

    fn marker(kind: RecordKind) -> Record {
        Record::new(1, Timestamp::UNIX_EPOCH, kind)
    }

    #[test]
    fn the_text_filter_ignores_case_on_both_sides() {
        let filter = LogFilter::new(StreamSet::ALL, Some("FaTaL"));

        assert!(filter.admits(&line("2026 [9] FATAL: bad password")));
        assert!(filter.admits(&line("fatal")));
        assert!(!filter.admits(&line("all fine")));
    }

    #[test]
    fn losses_always_pass_and_run_markers_only_without_text() {
        let filter = LogFilter::new(StreamSet::ALL, Some("x"));
        assert!(filter.admits(&marker(RecordKind::Truncated { bytes: 1 })));
        assert!(filter.admits(&marker(RecordKind::Unavailable)));
        assert!(!filter.admits(&marker(RecordKind::Started { pid: 1 })));
        assert!(!filter.admits(&marker(RecordKind::Exited {
            code: Some(0),
            signal: None
        })));

        assert!(LogFilter::all().admits(&marker(RecordKind::Started { pid: 1 })));
    }

    #[test]
    fn an_empty_stream_list_means_both() {
        assert_eq!(StreamSet::of(&[]), StreamSet::ALL);
        assert_eq!(
            StreamSet::of(&[OutputStream::Stderr]).streams(),
            [OutputStream::Stderr]
        );
    }
}
