//! The record format of the durable log store (spec 0027, *Format record*):
//! one JSON object per line, ended by `\n`, with short keys.
//!
//! ```json
//! {"seq":4812,"at":"2026-11-01T07:03:22.118Z","k":"line","text":"ready","level":"normal","src":{"pid":4123,"startedAt":"2026-11-01T07:03:21.904512Z","end":18432}}
//! {"seq":4813,"at":"2026-11-01T07:03:22.618Z","k":"truncated","bytes":1048576}
//! {"seq":4814,"at":"2026-11-01T07:05:00.002Z","k":"exited","code":0}
//! ```
//!
//! Unknown keys are ignored, so format version 1 can gain optional keys.

use jiff::{Timestamp, Unit};
use serde::{Deserialize, Serialize};

use super::level::LogLevel;

/// One record of a segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Unique for the instance and rising across both streams, process
    /// runs, and daemon restarts. A run marker carries the same `seq` in
    /// both streams.
    pub seq: u64,
    /// When the daemon read the line (or saw the event), to the
    /// millisecond, UTC.
    pub at: Timestamp,
    /// What the record holds.
    pub kind: RecordKind,
}

/// What a [`Record`] holds.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordKind {
    /// One piece of output.
    Line(LineRecord),
    /// Bytes of this stream that were dropped unread.
    Truncated {
        /// How many.
        bytes: u64,
    },
    /// A new main process started.
    Started {
        /// Its PID.
        pid: u32,
    },
    /// The main process exited.
    Exited {
        /// Its exit code, when it exited on its own.
        code: Option<i32>,
        /// The signal that ended it.
        signal: Option<i32>,
    },
    /// The output a process wrote while no daemon read it could not be
    /// read.
    Unavailable,
}

impl RecordKind {
    /// Returns whether the record is a run marker, written to both streams
    /// with one `seq`.
    #[must_use]
    pub const fn is_run_marker(&self) -> bool {
        matches!(
            self,
            Self::Started { .. } | Self::Exited { .. } | Self::Unavailable
        )
    }
}

/// One piece of output, already redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRecord {
    /// The text, without the line ending.
    pub text: String,
    /// The level from [`level_of`](super::level_of).
    pub level: LogLevel,
    /// The process had not ended the line yet.
    pub partial: bool,
    /// The piece continues the line of the piece before it.
    pub continued: bool,
    /// The init step whose output this is, by its number.
    pub step: Option<String>,
    /// Where in which spool the line ended, for dedupe; only on the last
    /// piece of a logical line read from a spool.
    pub src: Option<RecordSource>,
}

/// The spool position of a line (`src`): the process and the offset right
/// after the logical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordSource {
    /// The PID of the process.
    pub pid: u32,
    /// When the process started, to the microsecond.
    pub started_at: Timestamp,
    /// The spool offset right after the logical line.
    pub end: u64,
}

impl RecordSource {
    /// Returns whether `other` comes from the same process run.
    #[must_use]
    pub fn same_run(&self, other: &Self) -> bool {
        self.pid == other.pid && self.started_at == other.started_at
    }
}

/// The flat shape of a record on disk.
#[derive(Serialize, Deserialize)]
struct Wire {
    seq: u64,
    #[serde(with = "millis")]
    at: Timestamp,
    k: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    level: Option<LogLevel>,
    #[serde(default, skip_serializing_if = "is_false")]
    partial: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    cont: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    src: Option<RecordSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signal: Option<i32>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_false(value: &bool) -> bool {
    !*value
}

impl Wire {
    const fn bare(seq: u64, at: Timestamp, k: String) -> Self {
        Self {
            seq,
            at,
            k,
            text: None,
            level: None,
            partial: false,
            cont: false,
            step: None,
            src: None,
            bytes: None,
            pid: None,
            code: None,
            signal: None,
        }
    }
}

impl Record {
    /// Returns a record at `at` cut down to the millisecond.
    #[must_use]
    pub fn new(seq: u64, at: Timestamp, kind: RecordKind) -> Self {
        Self {
            seq,
            at: to_millis(at),
            kind,
        }
    }

    /// Encodes the record as one line, with its `\n`.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = serde_json::to_vec(&self.to_wire())
            .expect("a record of strings and numbers always serializes");
        out.push(b'\n');
        out
    }

    /// Decodes one line without its `\n`. Returns `None` for anything that
    /// is not a record of a kind this version knows.
    #[must_use]
    pub fn decode(line: &[u8]) -> Option<Self> {
        let wire: Wire = serde_json::from_slice(line).ok()?;
        let kind = match wire.k.as_str() {
            "line" => RecordKind::Line(LineRecord {
                text: wire.text?,
                level: wire.level.unwrap_or(LogLevel::Normal),
                partial: wire.partial,
                continued: wire.cont,
                step: wire.step,
                src: wire.src,
            }),
            "truncated" => RecordKind::Truncated { bytes: wire.bytes? },
            "started" => RecordKind::Started { pid: wire.pid? },
            "exited" => RecordKind::Exited {
                code: wire.code,
                signal: wire.signal,
            },
            "unavailable" => RecordKind::Unavailable,
            _ => return None,
        };
        Some(Self {
            seq: wire.seq,
            at: wire.at,
            kind,
        })
    }

    fn to_wire(&self) -> Wire {
        match &self.kind {
            RecordKind::Line(line) => Wire {
                text: Some(line.text.clone()),
                level: Some(line.level),
                partial: line.partial,
                cont: line.continued,
                step: line.step.clone(),
                src: line.src,
                ..Wire::bare(self.seq, self.at, "line".to_owned())
            },
            RecordKind::Truncated { bytes } => Wire {
                bytes: Some(*bytes),
                ..Wire::bare(self.seq, self.at, "truncated".to_owned())
            },
            RecordKind::Started { pid } => Wire {
                pid: Some(*pid),
                ..Wire::bare(self.seq, self.at, "started".to_owned())
            },
            RecordKind::Exited { code, signal } => Wire {
                code: *code,
                signal: *signal,
                ..Wire::bare(self.seq, self.at, "exited".to_owned())
            },
            RecordKind::Unavailable => Wire::bare(self.seq, self.at, "unavailable".to_owned()),
        }
    }
}

/// Cuts `at` down to the millisecond.
fn to_millis(at: Timestamp) -> Timestamp {
    at.round(
        jiff::TimestampRound::new()
            .smallest(Unit::Millisecond)
            .mode(jiff::RoundMode::Trunc),
    )
    .unwrap_or(at)
}

/// Reads the `seq` of an encoded record without decoding the rest, for the
/// binary search inside a segment.
#[must_use]
pub(crate) fn peek_seq(line: &[u8]) -> Option<u64> {
    const KEY: &[u8] = b"{\"seq\":";
    let rest = line.strip_prefix(KEY)?;
    let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    std::str::from_utf8(&rest[..digits]).ok()?.parse().ok()
}

/// `at` as RFC 3339 with exactly three fraction digits.
mod millis {
    use jiff::Timestamp;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        at: &Timestamp,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&at.strftime("%Y-%m-%dT%H:%M:%S%.3fZ"))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Timestamp, D::Error> {
        let text = <std::borrow::Cow<'_, str>>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> Timestamp {
        "2026-11-01T07:03:22.118765Z".parse().unwrap()
    }

    #[test]
    fn a_line_record_encodes_with_short_keys_and_milliseconds() {
        let record = Record::new(
            4812,
            at(),
            RecordKind::Line(LineRecord {
                text: "ready".to_owned(),
                level: LogLevel::Normal,
                partial: false,
                continued: false,
                step: None,
                src: Some(RecordSource {
                    pid: 4123,
                    started_at: "2026-11-01T07:03:21.904512Z".parse().unwrap(),
                    end: 18432,
                }),
            }),
        );

        let line = String::from_utf8(record.encode()).unwrap();

        assert_eq!(
            line,
            "{\"seq\":4812,\"at\":\"2026-11-01T07:03:22.118Z\",\"k\":\"line\",\"text\":\"ready\",\"level\":\"normal\",\"src\":{\"pid\":4123,\"startedAt\":\"2026-11-01T07:03:21.904512Z\",\"end\":18432}}\n"
        );
    }

    #[test]
    fn every_kind_round_trips() {
        let kinds = [
            RecordKind::Line(LineRecord {
                text: "x \"quoted\"".to_owned(),
                level: LogLevel::Error,
                partial: true,
                continued: true,
                step: Some("2".to_owned()),
                src: None,
            }),
            RecordKind::Truncated { bytes: 1_048_576 },
            RecordKind::Started { pid: 7 },
            RecordKind::Exited {
                code: None,
                signal: Some(9),
            },
            RecordKind::Unavailable,
        ];
        for (seq, kind) in kinds.into_iter().enumerate() {
            let record = Record::new(seq as u64 + 1, at(), kind);
            let encoded = record.encode();

            let decoded = Record::decode(&encoded[..encoded.len() - 1]).unwrap();

            assert_eq!(decoded, record);
            assert_eq!(peek_seq(&encoded), Some(record.seq));
        }
    }

    #[test]
    fn unknown_keys_are_ignored_and_unknown_kinds_or_garbage_are_not_records() {
        let extra = br#"{"seq":3,"at":"2026-11-01T07:03:22.118Z","k":"unavailable","future":1}"#;
        assert_eq!(Record::decode(extra).unwrap().kind, RecordKind::Unavailable);

        assert!(
            Record::decode(br#"{"seq":3,"at":"2026-11-01T07:03:22.118Z","k":"later"}"#).is_none()
        );
        assert!(Record::decode(b"{\"seq\":3,\"at\":").is_none());
        assert!(Record::decode(b"").is_none());
    }
}
